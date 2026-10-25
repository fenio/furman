use super::*;
use std::os::unix::fs::PermissionsExt;

fn preset(name: &str) -> ComparisonPreset {
    ComparisonPreset {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        compare_mode: ComparisonMode::Sha256,
        exclude_patterns: vec![],
    }
}

#[test]
fn missing_store_is_empty_and_roundtrips_only_explicit_options() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new/presets.json");
    let initial = load(&path).unwrap();
    assert_eq!(initial.revision, "missing");
    assert!(initial.presets.is_empty());
    assert!(!path.exists());
    let mut entry = preset("Content verification");
    entry.exclude_patterns = vec!["*.tmp".into(), "nested/**".into()];
    let result = save(&path, &initial.revision, vec![entry.clone()]).unwrap();
    assert_ne!(result.revision, "missing");
    assert_eq!(load(&path).unwrap().revision, result.revision);
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.presets[0].id, entry.id);
    assert_eq!(loaded.presets[0].exclude_patterns, entry.exclude_patterns);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 2);
    assert_eq!(json["presets"][0].as_object().unwrap().len(), 4);
    assert_eq!(json["presets"][0]["compare_mode"], "sha256");
}

#[test]
fn update_and_delete_require_current_revision_and_preserve_empty_exclusions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("presets.json");
    let mut entry = preset("Initial");
    let first = save(&path, "missing", vec![entry.clone()]).unwrap();
    entry.name = "Renamed".into();
    entry.compare_mode = ComparisonMode::SizeMtime;
    let second = save(&path, &first.revision, vec![entry]).unwrap();
    assert_ne!(first.revision, second.revision);
    let unchanged = fs::read(&path).unwrap();
    assert!(save(&path, &first.revision, vec![]).is_err());
    assert_eq!(fs::read(&path).unwrap(), unchanged);
    assert!(second.presets[0].exclude_patterns.is_empty());
    let empty = save(&path, &second.revision, vec![]).unwrap();
    assert!(empty.presets.is_empty());
    assert!(load(&path).unwrap().presets.is_empty());
}

#[test]
fn corrupt_unknown_and_future_files_are_never_silently_replaced() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("presets.json");
    let valid = serde_json::to_value(PresetFile {
        version: 1,
        presets: vec![preset("Valid")],
    })
    .unwrap();
    let mut future = valid.clone();
    future["version"] = 2.into();
    let mut secret = valid.clone();
    secret["presets"][0]["secret_key"] = "DO_NOT_EXPOSE".into();
    let mut policy = valid.clone();
    policy["presets"][0]["write_policy"] = "overwrite".into();
    let mut mode = valid.clone();
    mode["presets"][0]["compare_mode"] = "etag".into();
    for bytes in [
        b"invalid SECRET_CONTENT".to_vec(),
        b"".to_vec(),
        serde_json::to_vec(&future).unwrap(),
        serde_json::to_vec(&secret).unwrap(),
        serde_json::to_vec(&policy).unwrap(),
        serde_json::to_vec(&mode).unwrap(),
    ] {
        fs::write(&path, &bytes).unwrap();
        let failure = load(&path).err().unwrap().to_string();
        assert!(!failure.contains("DO_NOT_EXPOSE"));
        assert!(!failure.contains("SECRET_CONTENT"));
        assert!(save(&path, "missing", vec![]).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn bounded_validation_rejects_duplicates_invalid_names_and_excessive_patterns() {
    assert!(validate(&[preset(" Valid")]).is_err());
    assert!(validate(&[preset("")]).is_err());
    assert!(validate(&[preset("bad\nname")]).is_err());
    assert!(validate(&[preset(&"x".repeat(81))]).is_err());
    assert!(validate(&[preset("Same"), preset("same")]).is_err());
    let entry = preset("Same ID");
    let mut duplicate = entry.clone();
    duplicate.name = "Other".into();
    assert!(validate(&[entry, duplicate]).is_err());
    let mut entry = preset("Bad ID");
    entry.id = "../outside".into();
    assert!(validate(&[entry]).is_err());
    for patterns in [
        vec!["".into()],
        vec!["with,comma".into()],
        vec![" spaced ".into()],
        vec!["\n".into()],
        vec!["x".repeat(513)],
        vec!["*.tmp".into(); 129],
    ] {
        let mut entry = preset("Patterns");
        entry.exclude_patterns = patterns;
        assert!(validate(&[entry]).is_err());
    }
    assert!(validate(
        &(0..101)
            .map(|index| preset(&format!("p{index}")))
            .collect::<Vec<_>>()
    )
    .is_err());
    assert!(validate(&[preset("日本語 — explicit empty exclusions")]).is_ok());
}

#[test]
fn oversized_symlink_directory_and_fifo_storage_fail_closed_without_blocking() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("presets.json");
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(load(&path).is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(load(&path).is_err());
    fs::remove_dir(&path).unwrap();
    let target = root.path().join("target");
    fs::write(&target, "PRIVATE").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(load(&path).is_err());
    assert!(save(&path, "missing", vec![]).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"PRIVATE");
    fs::remove_file(&path).unwrap();
    nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
    assert!(load(&path).is_err());
}

/// A child forked by a concurrent test can briefly inherit a lock fd and keep
/// the lock held until it execs, so contention is retried instead of failing.
fn retry_while_locked<T, E>(mut attempt: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    for _ in 0..50 {
        if let Ok(value) = attempt() {
            return Ok(value);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    attempt()
}

#[test]
fn cooperating_process_lock_prevents_write_and_revision_races() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("presets.json");
    let initial = save(&path, "missing", vec![preset("Original")]).unwrap();
    let before = fs::read(&path).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .unwrap();
    retry_while_locked(|| lock.try_lock()).unwrap();
    assert!(save(&path, &initial.revision, vec![])
        .err()
        .unwrap()
        .to_string()
        .contains("Another process"));
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(lock);
    retry_while_locked(|| save(&path, &initial.revision, vec![])).unwrap();
}

#[test]
fn failed_validation_size_limits_or_replace_leave_previous_file_intact() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("presets.json");
    let initial = save(&path, "missing", vec![preset("Original")]).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(save(&path, &initial.revision, vec![preset("")]).is_err());
    let many = (0..100)
        .map(|index| {
            let mut entry = preset(&format!("p{index}"));
            entry.exclude_patterns = vec!["x".repeat(512); 128];
            entry
        })
        .collect();
    assert!(save(&path, &initial.revision, many).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let blocked = root.path().join("parent-is-a-file");
    fs::write(&blocked, "unchanged").unwrap();
    assert!(save(&blocked.join("presets.json"), "missing", vec![]).is_err());
    assert_eq!(fs::read(&blocked).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3); // Store, lock and fixture; no leaked temporary files.
}
