use crate::models::FmError;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

/// Managed state that holds one `RecommendedWatcher` per watched directory,
/// keyed by a caller-supplied id string.
pub struct WatcherState(pub Mutex<HashMap<String, RecommendedWatcher>>);

/// Simplified fs-change event payload that is Serialize-friendly.
#[derive(Debug, Clone, Serialize)]
pub struct FsChangeEvent {
    pub kind: String,
    pub paths: Vec<String>,
}

impl From<&Event> for FsChangeEvent {
    fn from(event: &Event) -> Self {
        FsChangeEvent {
            kind: format!("{:?}", event.kind),
            paths: event.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        }
    }
}

/// Start watching a directory for file-system changes.
///
/// Change events are emitted to the front-end as `"fs-change"` events.
#[tauri::command]
pub fn watch_directory(
    path: String,
    id: String,
    app_handle: AppHandle,
    state: State<'_, WatcherState>,
) -> Result<(), FmError> {
    let dir = PathBuf::from(&path);
    if !dir.exists() {
        return Err(FmError::NotFound(path));
    }

    // Build a new watcher that forwards events via the Tauri event bus.
    let handle = app_handle.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                let payload = FsChangeEvent::from(&event);
                let _ = handle.emit("fs-change", &payload);
            }
            Err(e) => {
                log::warn!("fs watcher error: {e}");
            }
        }
    })?;

    watcher.watch(&dir, RecursiveMode::NonRecursive)?;

    let mut map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;
    map.insert(id, watcher);

    Ok(())
}

/// Stop watching a previously-watched directory identified by `id`.
#[tauri::command]
pub fn unwatch_directory(
    id: String,
    state: State<'_, WatcherState>,
) -> Result<(), FmError> {
    let mut map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(format!("lock poisoned: {e}")))?;

    // Removing the watcher from the map drops it, which stops watching.
    if map.remove(&id).is_none() {
        return Err(FmError::NotFound(format!("no watcher with id: {id}")));
    }

    Ok(())
}
