//! Options-only presets: never persist endpoints, credentials, paths, scan
//! results, selections, write policies or approvals. No execution entry points.
use crate::models::FmError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::State;

const MAX_BYTES: u64 = 256 * 1024;
const MAX_PRESETS: usize = 100;

fn error(message: &str) -> FmError {
    FmError::Other(message.into())
}
fn io_error(action: &str, failure: std::io::Error) -> FmError {
    error(&format!("Could not {action} comparison presets ({:?}). Check local configuration directory permissions; existing data was not intentionally discarded.", failure.kind()))
}

fn revision(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonPreset {
    pub id: String,
    pub name: String,
    pub compare_mode: ComparisonMode,
    pub exclude_patterns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonMode {
    Sha256,
    SizeMtime,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    version: u32,
    presets: Vec<ComparisonPreset>,
}

#[derive(Serialize)]
pub struct PresetReport {
    pub revision: String,
    pub presets: Vec<ComparisonPreset>,
}

#[derive(Default)]
pub struct ComparisonPresetState(Mutex<()>);

fn storage_path() -> Result<PathBuf, FmError> {
    Ok(dirs::home_dir()
        .ok_or_else(|| {
            error("Home directory unavailable; comparison presets cannot be loaded or saved")
        })?
        .join(".config/furman/comparison-presets.json"))
}

fn validate(presets: &[ComparisonPreset]) -> Result<(), FmError> {
    if presets.len() > MAX_PRESETS {
        return Err(error("At most 100 comparison presets can be saved"));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for preset in presets {
        if uuid::Uuid::parse_str(&preset.id).is_err() || !ids.insert(&preset.id) {
            return Err(error("Comparison presets contain invalid or duplicate IDs"));
        }
        if preset.name.is_empty()
            || preset.name != preset.name.trim()
            || preset.name.chars().count() > 80
            || preset.name.chars().any(char::is_control)
        {
            return Err(error("Preset names must contain 1–80 characters, without surrounding whitespace or control characters"));
        }
        if !names.insert(preset.name.to_lowercase()) {
            return Err(error("Preset names must be unique (ignoring case); use Update selected to replace an existing preset"));
        }
        if preset.exclude_patterns.len() > 128
            || preset.exclude_patterns.iter().any(|pattern| {
                pattern.is_empty()
                    || pattern != pattern.trim()
                    || pattern.len() > 512
                    || pattern.contains(',')
                    || pattern.chars().any(char::is_control)
            })
        {
            return Err(error("Use at most 128 explicit exclusions, each 1–512 bytes without commas, surrounding whitespace or control characters"));
        }
    }
    Ok(())
}

fn load(path: &Path) -> Result<PresetReport, FmError> {
    // A malformed, unsupported, unreadable or nonregular file must not be
    // mistaken for an empty store and subsequently overwritten by the UI.
    match fs::symlink_metadata(path) {
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(PresetReport { revision: "missing".into(), presets: vec![] }),
        Err(failure) => return Err(io_error("inspect", failure)),
        Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_BYTES => return Err(error("Comparison preset storage must be a regular file no larger than 256 KiB; it was not replaced")),
        Ok(_) => {}
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|failure| io_error("open", failure))?;
    if !file
        .metadata()
        .map_err(|failure| io_error("inspect opened", failure))?
        .is_file()
    {
        return Err(error("Comparison preset storage must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|failure| io_error("read", failure))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error(
            "Comparison preset storage exceeds 256 KiB; it was not replaced",
        ));
    }
    let document: PresetFile = serde_json::from_slice(&bytes).map_err(|_| error("Comparison preset storage is invalid or contains unsupported fields. No file contents were included; repair or back up the file externally before retrying."))?;
    if document.version != 1 {
        return Err(error("Unsupported comparison preset file version; upgrade Furman or review the file externally. It was not replaced."));
    }
    validate(&document.presets)?;
    Ok(PresetReport {
        revision: revision(&bytes),
        presets: document.presets,
    })
}

fn save(
    path: &Path,
    expected_revision: &str,
    presets: Vec<ComparisonPreset>,
) -> Result<PresetReport, FmError> {
    validate(&presets)?;
    let parent = path
        .parent()
        .ok_or_else(|| error("Comparison preset storage directory unavailable"))?;
    fs::create_dir_all(parent)
        .map_err(|failure| io_error("create storage directory for", failure))?;
    // Cooperating Furman processes share an advisory lock. Never wait for a
    // stalled process, and re-read the revision only after acquiring the lock.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path.with_extension("lock"))
        .map_err(|failure| io_error("open storage lock for", failure))?;
    if !lock
        .metadata()
        .map_err(|failure| io_error("inspect storage lock for", failure))?
        .is_file()
    {
        return Err(error(
            "Comparison preset storage lock must be a regular file",
        ));
    }
    lock.try_lock().map_err(|_| error("Another process is saving comparison presets, or storage locking is unavailable. Retry after it finishes; no changes were written."))?;
    let current = load(path)?;
    if current.revision != expected_revision {
        return Err(error("Comparison presets changed since loading. Refresh presets and review the current list before saving again; no changes were written."));
    }
    let document = PresetFile {
        version: 1,
        presets,
    };
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| error("Could not encode comparison presets"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error(
            "Combined comparison presets exceed the 256 KiB storage limit",
        ));
    }
    // Same-directory atomic replacement. Tempfile defaults to mode 0600 on Unix;
    // failed writes leave the last complete preset file intact.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|failure| io_error("create temporary storage for", failure))?;
    temporary
        .write_all(&bytes)
        .map_err(|failure| io_error("write", failure))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|failure| io_error("flush", failure))?;
    temporary
        .persist(path)
        .map_err(|failure| io_error("replace", failure.error))?;
    // Return the committed document directly; no second read can misreport a
    // successful save as failure or return an unrelated external replacement.
    Ok(PresetReport {
        revision: revision(&bytes),
        presets: document.presets,
    })
}

#[tauri::command]
pub fn comparison_presets_load(
    state: State<'_, ComparisonPresetState>,
) -> Result<PresetReport, FmError> {
    let _guard = state
        .0
        .lock()
        .map_err(|_| error("Comparison preset storage is busy or unavailable"))?;
    load(&storage_path()?)
}

#[tauri::command]
pub fn comparison_presets_save(
    state: State<'_, ComparisonPresetState>,
    expected_revision: String,
    presets: Vec<ComparisonPreset>,
) -> Result<PresetReport, FmError> {
    let _guard = state
        .0
        .lock()
        .map_err(|_| error("Comparison preset storage is busy or unavailable"))?;
    save(&storage_path()?, &expected_revision, presets)
}

#[cfg(test)]
#[path = "comparison_presets_tests.rs"]
mod tests;
