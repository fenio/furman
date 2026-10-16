use super::*;
use crate::job_runs::{
    tests::{bind_fixture, dependencies, empty, fixture, replay},
    JobRunState,
};
use std::{os::unix::fs::symlink, sync::atomic::Ordering};

fn record(job: &str, status: Status) -> Record {
    Record {
        run_id: fresh_id(),
        job_id: job.into(),
        job_revision: "1".into(),
        started_ms: 1,
        updated_ms: 1,
        status,
        binding: Counters::default(),
        comparison: Counters::default(),
        preparation: Counters::default(),
        policy: None,
        review: Counters::default(),
        execution: None,
    }
}
fn raw_path(paths: &Paths) -> PathBuf {
    paths.store.parent().unwrap().join("job-runs/journal.json")
}
fn sample_progress() -> ScanProgress {
    ScanProgress {
        scanned: 3,
        requests: 2,
        bytes_read: 6,
        s3_bytes_read: 3,
        elapsed_ms: 10,
        ..ScanProgress::default()
    }
}

#[test]
fn v1_cannot_smuggle_executable_intents_and_v2_requires_complete_native_counters() {
    let mut store = Store {
        revision: fresh_id(),
        version: 1,
        ..Store::default()
    };
    let mut intent = record(&fresh_id(), Status::Executing);
    intent.policy = Some(Policy::CreateOnly);
    intent.execution = Some(Execution {
        selected_files: 1,
        selected_bytes: 3,
        ..Execution::default()
    });
    store.records.push(intent);
    assert!(store.validate(true).is_err());
    store.version = 2;
    assert!(store.validate(true).is_ok());
    store.records[0].status = Status::Succeeded;
    assert!(store.validate(true).is_err());
    let counts = store.records[0].execution.as_mut().unwrap();
    counts.attempted_files = 1;
    counts.acknowledged_files = 1;
    counts.verified_files = 1;
    counts.verified_bytes = 3;
    store.validate(true).unwrap();
    let mut value = serde_json::to_value(&store).unwrap();
    value["records"][0]["execution"]["key"] = "caller-key".into();
    assert!(serde_json::from_value::<Store>(value).is_err());
}

#[test]
fn v1_read_only_history_migrates_without_replay_and_executing_recovery_preserves_verified_and_unknown_counts(
) {
    let (_root, paths, job) = fixture(true, false);
    let run = fresh_id();
    start(&paths, &run, &job).unwrap();
    let path = raw_path(&paths);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["version"] = 1.into();
    value["records"][0]
        .as_object_mut()
        .unwrap()
        .remove("review");
    value["records"][0]
        .as_object_mut()
        .unwrap()
        .remove("execution");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let result = recover(&paths).unwrap();
    assert_eq!(result.records[0].status, Status::Interrupted);
    assert!(result.records[0].execution.is_none());
    assert_eq!(
        serde_json::from_slice::<Store>(&fs::read(&path).unwrap())
            .unwrap()
            .version,
        2
    );
    let storage = Storage::open(&paths, false).unwrap().unwrap();
    let lock = storage.lock().unwrap();
    let (mut store, _) = storage.load().unwrap();
    store.records[0].status = Status::Executing;
    store.records[0].policy = Some(Policy::CreateOnly);
    store.records[0].execution = Some(Execution {
        selected_files: 3,
        selected_bytes: 9,
        attempted_files: 2,
        acknowledged_files: 1,
        verified_files: 1,
        verified_bytes: 3,
        uncertain_files: 0,
        skipped_files: 0,
    });
    store.pauses.clear();
    storage.commit(&mut store).unwrap();
    drop(lock);
    let state = JobRunState::default();
    let recovered = state.history(&paths).unwrap();
    let counts = recovered.records[0].execution.as_ref().unwrap();
    assert_eq!(counts.verified_files, 1);
    assert_eq!(counts.uncertain_files, 1);
    assert_eq!(recovered.records[0].status, Status::Interrupted);
    assert!(!recovered.pauses.is_empty());
    assert!(state.0.lock().unwrap().slot.is_none());
    assert!(state.status(&run).is_err());
    assert_eq!(state.history(&paths).unwrap().revision, recovered.revision);
}

#[test]
fn native_records_are_private_strict_and_contain_no_namespace_payload_or_credentials() {
    let (_root, paths, job) = fixture(true, false);
    let lease = super::super::run_lock(&paths).unwrap();
    let run = fresh_id();
    start(&paths, &run, &job).unwrap();
    transition(&paths, &run, Status::Bound, sample_progress(), None).unwrap();
    transition(
        &paths,
        &run,
        Status::Scanning,
        ScanProgress::default(),
        None,
    )
    .unwrap();
    transition(&paths, &run, Status::Compared, sample_progress(), None).unwrap();
    transition(
        &paths,
        &run,
        Status::Preparing,
        ScanProgress::default(),
        Some(Policy::ReplaceUnchanged),
    )
    .unwrap();
    transition(&paths, &run, Status::Prepared, sample_progress(), None).unwrap();
    transition(&paths, &run, Status::Discarded, sample_progress(), None).unwrap();
    let bytes = fs::read(raw_path(&paths)).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for secret in [
        "job.test",
        "destination-bucket",
        "source-bucket",
        "local-source",
        "src//",
        "dest//",
        "FAKE",
        "sha256",
        "etag",
        "approval",
        "payload",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert_eq!(
        fs::metadata(raw_path(&paths)).unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(raw_path(&paths).parent().unwrap())
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    let report = read(&paths).unwrap();
    assert_eq!(report.records.len(), 1);
    assert_eq!(report.records[0].status, Status::Discarded);
    assert_eq!(report.records[0].comparison.bytes_read, 6);
    assert_eq!(report.records[0].preparation.s3_bytes_read, 3);
    assert!(!report.execution_available);
    assert!(!raw_path(&paths).parent().unwrap().join("dirty").exists());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["records"][0]["comparison"]["credential"] = "DO_NOT_EXPOSE".into();
    fs::write(raw_path(&paths), serde_json::to_vec(&value).unwrap()).unwrap();
    let failure = read(&paths).err().unwrap().to_string();
    assert!(!failure.contains("DO_NOT_EXPOSE"));
    assert!(read(&paths).is_err());
    drop(lease);
}

#[test]
fn exclusive_recovery_is_local_idempotent_and_review_is_revision_checked() {
    let (_root, paths, job) = fixture(true, false);
    let lease = super::super::run_lock(&paths).unwrap();
    let run = fresh_id();
    start(&paths, &run, &job).unwrap();
    transition(&paths, &run, Status::Bound, sample_progress(), None).unwrap();
    let state = JobRunState::default();
    let live = state.history(&paths).unwrap();
    assert!(live.reconciliation_deferred);
    assert_eq!(live.records[0].status, Status::Bound);
    assert!(state
        .review_history(&paths, &live.revision, None, true, true)
        .is_err());
    drop(lease);
    let recovered = state.history(&paths).unwrap();
    assert_eq!(recovered.records[0].status, Status::Interrupted);
    assert_eq!(recovered.records[0].binding.requests, 2);
    assert_eq!(recovered.pauses[&job.id].reason, Status::Interrupted);
    assert!(state.0.lock().unwrap().slot.is_none());
    assert_eq!(state.history(&paths).unwrap().revision, recovered.revision);
    assert!(ensure_ready(&paths, &job.id).is_err());
    assert!(state
        .review_history(&paths, &live.revision, Some(&job.id), true, false)
        .is_err());
    assert!(state
        .review_history(&paths, &recovered.revision, Some(&job.id), false, false)
        .is_err());
    assert!(state
        .review_history(&paths, &recovered.revision, None, true, true)
        .is_err());
    let reviewed = state
        .review_history(&paths, &recovered.revision, Some(&job.id), true, false)
        .unwrap();
    assert!(reviewed.pauses.is_empty());
    assert_eq!(reviewed.records[0].status, Status::Interrupted);
    assert!(ensure_ready(&paths, &job.id).is_ok());
    let cleared = state
        .review_history(&paths, &reviewed.revision, None, true, true)
        .unwrap();
    assert!(cleared.records.is_empty());
    assert!(crate::jobs::list(&paths).unwrap().jobs.len() == 1);
}

#[test]
fn every_durability_checkpoint_preserves_a_valid_old_or_new_record_and_requires_recovery() {
    for checkpoint in [
        "before_marker",
        "marker_durable",
        "file_written",
        "file_durable",
        "renamed",
        "directory_durable",
        "initialized_durable",
        "marker_removed",
        "finished",
    ] {
        let (_root, paths, job) = fixture(true, false);
        let run = fresh_id();
        start(&paths, &run, &job).unwrap();
        transition(&paths, &run, Status::Bound, sample_progress(), None).unwrap();
        let storage = Storage::open(&paths, false).unwrap().unwrap();
        let lock = storage.lock().unwrap();
        let (mut store, _) = storage.load().unwrap();
        store.records[0].status = Status::Scanning;
        assert!(storage
            .commit_with(&mut store, &|point| if point == checkpoint {
                Err(unavailable())
            } else {
                Ok(())
            })
            .is_err());
        drop(lock);
        let (visible, dirty) = storage.load().unwrap();
        assert!(matches!(
            visible.records[0].status,
            Status::Bound | Status::Scanning
        ));
        let result = recover(&paths).unwrap();
        assert_eq!(result.records[0].status, Status::Interrupted);
        assert!(result.pauses.contains_key(&job.id));
        assert_eq!(result.storage_review_required, dirty);
        assert!(!storage.dir.join("dirty").exists());
        let stable = fs::read(raw_path(&paths)).unwrap();
        assert_eq!(recover(&paths).unwrap().revision, result.revision);
        assert_eq!(fs::read(raw_path(&paths)).unwrap(), stable);
    }
}

#[test]
fn missing_initialized_history_cannot_hide_an_unresolved_attempt() {
    let (_root, paths, job) = fixture(true, false);
    start(&paths, &fresh_id(), &job).unwrap();
    fs::remove_file(raw_path(&paths)).unwrap();
    let failure = recover(&paths).err().unwrap().to_string();
    assert!(failure.contains("previously initialized"));
    assert!(!raw_path(&paths).exists());
    assert!(read(&paths).is_err());
}

#[test]
fn corrupt_future_oversized_symlink_fifo_shared_and_invalid_marker_storage_fail_closed() {
    for kind in 0..9 {
        let (root, paths, job) = fixture(true, false);
        start(&paths, &fresh_id(), &job).unwrap();
        let path = raw_path(&paths);
        let original = fs::read(&path).unwrap();
        match kind {
            0 => fs::write(&path, b"{DO_NOT_INCLUDE_RAW_DATA").unwrap(),
            1 => {
                let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                value["version"] = 3.into();
                fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            2 => {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len(MAX_BYTES + 1)
                    .unwrap();
            }
            3 => {
                fs::remove_file(&path).unwrap();
                symlink(root.path().join("elsewhere"), &path).unwrap();
            }
            4 => {
                fs::remove_file(&path).unwrap();
                nix::unistd::mkfifo(
                    &path,
                    nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
                )
                .unwrap();
            }
            5 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            6 => fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o755))
                .unwrap(),
            7 => {
                let marker = path.parent().unwrap().join("dirty");
                fs::write(&marker, DIRTY).unwrap();
                fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
                fs::write(marker, b"invalid-marker").unwrap();
            }
            _ => {
                let lock = path.parent().unwrap().join("lock");
                fs::remove_file(&lock).unwrap();
                symlink(root.path().join("other-lock"), lock).unwrap();
            }
        }
        let error = recover(&paths).err().unwrap().to_string();
        assert!(!error.contains("DO_NOT_INCLUDE_RAW_DATA"));
        assert!(read(&paths).is_err());
        assert!(raw_path(&paths).symlink_metadata().is_ok());
    }
}

#[test]
fn journal_lock_wait_is_bounded_explicitly_unlocked_and_directory_identity_is_pinned() {
    let (root, paths, job) = fixture(true, false);
    start(&paths, &fresh_id(), &job).unwrap();
    let storage = Storage::open(&paths, false).unwrap().unwrap();
    let owner = storage.lock().unwrap();
    let inherited = owner.0.try_clone().unwrap();
    assert!(read(&paths).is_err());
    drop(owner);
    assert!(read(&paths).is_ok());
    drop(inherited);
    let old = fs::read(raw_path(&paths)).unwrap();
    let renamed = root.path().join("old-journal-dir");
    fs::rename(&storage.dir, &renamed).unwrap();
    fs::create_dir(&storage.dir).unwrap();
    fs::set_permissions(&storage.dir, fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = Store::default();
    assert!(storage.commit(&mut store).is_err());
    assert_eq!(fs::read(renamed.join("journal.json")).unwrap(), old);
    assert!(!storage.dir.join("journal.json").exists());
}

#[test]
fn ordinary_short_metadata_reader_contention_does_not_abort_a_stage_commit() {
    let (_root, paths, job) = fixture(true, false);
    let run = fresh_id();
    start(&paths, &run, &job).unwrap();
    let storage = Storage::open(&paths, false).unwrap().unwrap();
    let owner = storage.lock().unwrap();
    let reader = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(owner);
    });
    transition(&paths, &run, Status::Bound, sample_progress(), None).unwrap();
    reader.join().unwrap();
    assert_eq!(read(&paths).unwrap().records[0].status, Status::Bound);
}

#[test]
fn retention_evicts_only_old_resolved_terminal_records_and_never_active_or_paused_intents() {
    let jobs: Vec<_> = (0..10).map(|_| fresh_id()).collect();
    let mut store = Store::default();
    store.revision = fresh_id();
    for job in &jobs {
        for _ in 0..50 {
            store.records.push(record(job, Status::Discarded));
        }
    }
    let protected = store.records[0].run_id.clone();
    store.records[0].status = Status::Failed;
    store.pauses.insert(
        jobs[0].clone(),
        Pause {
            run_id: protected.clone(),
            reason: Status::Failed,
        },
    );
    store.validate(true).unwrap();
    store.prune(&jobs[0]).unwrap();
    assert_eq!(store.records.len(), 499);
    assert!(store.records.iter().any(|r| r.run_id == protected));
    assert_eq!(
        store.records.iter().filter(|r| r.job_id == jobs[0]).count(),
        49
    );
    let mut full = Store::default();
    for _ in 0..500 {
        let job = fresh_id();
        let r = record(&job, Status::Interrupted);
        full.pauses.insert(
            job,
            Pause {
                run_id: r.run_id.clone(),
                reason: Status::Interrupted,
            },
        );
        full.records.push(r);
    }
    full.validate(false).unwrap();
    assert!(full.prune(&fresh_id()).is_err());
    assert_eq!(full.records.len(), 500);
}

#[tokio::test]
async fn failed_stage_recording_invalidates_native_handles_and_blocks_network_until_explicit_review(
) {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let original = fs::read(raw_path(&paths)).unwrap();
    fs::write(raw_path(&paths), b"UNREADABLE_PRIVATE_CONTENT").unwrap();
    let failure = crate::job_runs::scan(&state, &bound.run_id, Arc::new(|_| {}))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(failure.contains("JOB_JOURNAL"));
    assert!(!failure.contains("UNREADABLE_PRIVATE_CONTENT"));
    let stopped = state.status(&bound.run_id).unwrap();
    assert_eq!(stopped.phase, "stopped");
    assert!(stopped.manifest_id.is_none());
    assert!(stopped.preview.is_none());
    assert_eq!(http.actual_requests().count(), 1);
    assert!(state
        .0
        .lock()
        .unwrap()
        .journal_fault
        .load(Ordering::Relaxed));
    let (reader, factory, reads) = dependencies(replay(vec![]));
    assert!(super::super::bind_with(
        &state,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        true,
        reader,
        factory
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    assert_eq!(
        fs::read(raw_path(&paths)).unwrap(),
        b"UNREADABLE_PRIVATE_CONTENT"
    );
    fs::write(raw_path(&paths), original).unwrap();
    let history = state.history(&paths).unwrap();
    assert_eq!(history.records[0].status, Status::Interrupted);
    let reviewed = state
        .review_history(&paths, &history.revision, None, true, false)
        .unwrap();
    state
        .review_history(&paths, &reviewed.revision, Some(&job.id), true, false)
        .unwrap();
    let next = bind_fixture(&state, &paths, &job, &replay(vec![empty()])).await;
    assert_ne!(bound.run_id, next.run_id);
    state.release(&next.run_id).unwrap();
}

#[tokio::test]
async fn active_definition_deletion_is_refused_and_terminal_history_survives_definition_deletion() {
    let (_root, paths, job) = fixture(true, false);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &replay(vec![empty()])).await;
    let report = crate::jobs::list(&paths).unwrap();
    assert!(crate::jobs::delete(&paths, &report.revision, &job.id, true).is_err());
    assert_eq!(crate::jobs::list(&paths).unwrap().jobs.len(), 1);
    state.release(&bound.run_id).unwrap();
    crate::jobs::delete(&paths, &report.revision, &job.id, true).unwrap();
    let history = state.history(&paths).unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].status, Status::Cancelled);
    assert_eq!(history.records[0].job_id, job.id);
}

#[test]
fn interrupted_recovery_preserves_preparation_policy_but_never_restores_native_resources() {
    let (_root, paths, job) = fixture(true, false);
    let run = fresh_id();
    start(&paths, &run, &job).unwrap();
    transition(&paths, &run, Status::Bound, sample_progress(), None).unwrap();
    transition(&paths, &run, Status::Scanning, sample_progress(), None).unwrap();
    transition(&paths, &run, Status::Compared, sample_progress(), None).unwrap();
    transition(
        &paths,
        &run,
        Status::Preparing,
        sample_progress(),
        Some(Policy::ReplaceUnchanged),
    )
    .unwrap();
    transition(&paths, &run, Status::Prepared, sample_progress(), None).unwrap();
    let state = JobRunState::default();
    let report = state.history(&paths).unwrap();
    assert_eq!(report.records[0].status, Status::Interrupted);
    assert_eq!(report.records[0].policy, Some(Policy::ReplaceUnchanged));
    assert_eq!(report.records[0].preparation.s3_bytes_read, 3);
    assert!(!report.execution_available);
    assert!(state.0.lock().unwrap().slot.is_none());
    assert!(state.status(&run).is_err());
}

#[tokio::test]
async fn unavailable_initial_journal_blocks_keychain_network_and_requires_health_review_after_repair(
) {
    let (_root, paths, job) = fixture(true, false);
    let dir = raw_path(&paths).parent().unwrap().to_path_buf();
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::create_dir(dir.join("dirty")).unwrap();
    let http = replay(vec![]);
    let state = JobRunState::default();
    let (reader, factory, reads) = dependencies(http.clone());
    assert!(super::super::bind_with(
        &state,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        true,
        reader,
        factory
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    assert_eq!(http.actual_requests().count(), 0);
    assert!(state
        .0
        .lock()
        .unwrap()
        .journal_fault
        .load(Ordering::Relaxed));
    assert!(!raw_path(&paths).exists());
    fs::remove_dir(dir.join("dirty")).unwrap();
    let history = state.history(&paths).unwrap();
    assert!(history.in_memory_storage_fault);
    assert!(history.records.is_empty());
    assert!(state
        .review_history(&paths, &history.revision, None, true, true)
        .is_err());
    let healthy = state
        .review_history(&paths, &history.revision, None, true, false)
        .unwrap();
    assert!(!healthy.in_memory_storage_fault);
    let next = bind_fixture(&state, &paths, &job, &replay(vec![empty()])).await;
    state.release(&next.run_id).unwrap();
}

#[tokio::test]
async fn journal_failure_on_discard_removes_prepared_authority_even_when_outcome_cannot_be_saved() {
    for read_failure in [false, true] {
        use crate::job_runs::{preview, scan, tests::listing};
        use aws_smithy_types::body::SdkBody;
        use tauri::http::Response;
        let (_root, paths, job) = fixture(true, false);
        let super::super::SourcePin::Local { path, .. } = &job.source else {
            panic!()
        };
        fs::write(std::path::Path::new(path).join("file"), "abc").unwrap();
        let missing = Response::builder()
            .status(404)
            .body(SdkBody::from("<Error><Code>NoSuchKey</Code></Error>"))
            .unwrap();
        let http = replay(vec![empty(), listing("dest//", None), missing]);
        let state = JobRunState::default();
        let bound = bind_fixture(&state, &paths, &job, &http).await;
        let comparison = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
        let rows = state
            .page(&bound.run_id, comparison.manifest_id.as_deref().unwrap(), 0)
            .unwrap();
        let prepared = preview::prepare(
            &state,
            preview::Request {
                run_id: bound.run_id.clone(),
                manifest_id: comparison.manifest_id.unwrap(),
                entry_ids: vec![rows[0].id.clone()],
                policy: Policy::CreateOnly,
                max_snapshot_bytes: 100,
                approved: true,
            },
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(prepared.phase, "prepared");
        let original = fs::read(raw_path(&paths)).unwrap();
        fs::write(raw_path(&paths), b"CORRUPT_HISTORY_NO_RAW_DIAGNOSTICS").unwrap();
        if read_failure {
            assert!(state.history(&paths).is_err());
        } else {
            assert!(state.release(&bound.run_id).is_err());
        }
        let stopped = state.status(&bound.run_id).unwrap();
        assert!(stopped.preview.is_none());
        assert!(stopped.manifest_id.is_none());
        assert_eq!(stopped.phase, "stopped");
        assert!(stopped
            .message
            .unwrap()
            .contains("Outcome could not be recorded"));
        assert_eq!(http.actual_requests().count(), 3);
        fs::write(raw_path(&paths), original).unwrap();
        let recovered = state.history(&paths).unwrap();
        assert_eq!(recovered.records[0].status, Status::Interrupted);
        assert_eq!(recovered.records[0].policy, Some(Policy::CreateOnly));
        assert!(recovered.pauses.contains_key(&job.id));
    }
}

#[tokio::test]
async fn persisted_job_review_pause_never_reads_keys_and_is_not_mistaken_for_global_storage_failure(
) {
    let (_root, paths, job) = fixture(true, false);
    let id = fresh_id();
    start(&paths, &id, &job).unwrap();
    transition(&paths, &id, Status::Failed, ScanProgress::default(), None).unwrap();
    let state = JobRunState::default();
    let (reader, factory, reads) = dependencies(replay(vec![]));
    let result = super::super::bind_with(
        &state,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        true,
        reader,
        factory,
    )
    .await;
    assert!(result.is_err());
    assert!(reads.lock().unwrap().is_empty());
    assert!(!state
        .0
        .lock()
        .unwrap()
        .journal_fault
        .load(Ordering::Relaxed));
    let report = state.history(&paths).unwrap();
    assert_eq!(report.records.len(), 1);
    assert!(report.pauses.contains_key(&job.id));
}
