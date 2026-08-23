use crate::models::{FmError, SearchDone, SearchEvent, SearchResult};
use crate::search_matcher::SearchMatcher;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

/// Maximum file size (bytes) to search content in.
const MAX_CONTENT_FILE_SIZE: u64 = 5 * 1024 * 1024; // 5 MB

/// Maximum number of results sent to the frontend.
const MAX_STREAMED_RESULTS: u32 = 1000;

// ── Managed state ───────────────────────────────────────────────────────────

pub struct SearchState(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn search_files(
    id: String,
    root: String,
    query: String,
    mode: String, // "name" | "content"
    use_regex: bool,
    channel: Channel<SearchEvent>,
    state: tauri::State<'_, SearchState>,
) -> Result<(), FmError> {
    let matcher = SearchMatcher::new(&query, use_regex)?;
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), cancel_flag.clone());
    }

    // Spawn a thread for the blocking directory walk.
    // The channel streams results back to the frontend as they're found.
    std::thread::spawn(move || {
        do_search(&root, &mode, &matcher, &channel, &cancel_flag);
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_search(id: String, state: tauri::State<'_, SearchState>) -> Result<(), FmError> {
    let map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flag) = map.get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ── Search implementation ───────────────────────────────────────────────────

fn do_search(
    root: &str,
    mode: &str,
    matcher: &SearchMatcher,
    channel: &Channel<SearchEvent>,
    cancel_flag: &Arc<AtomicBool>,
) {
    let is_content = mode == "content";
    let root_path = PathBuf::from(root);

    let mut stack: Vec<PathBuf> = vec![root_path];
    let mut total_found: u32 = 0;
    let mut streamed: u32 = 0;

    while let Some(dir) = stack.pop() {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = channel.send(SearchEvent::Done(SearchDone {
                total_found,
                cancelled: true,
            }));
            return;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue, // permission denied, etc.
        };

        for entry in entries.flatten() {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = channel.send(SearchEvent::Done(SearchDone {
                    total_found,
                    cancelled: true,
                }));
                return;
            }

            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Skip symlink directories to avoid loops.
            if metadata.is_symlink() && path.is_dir() {
                continue;
            }

            let is_dir = metadata.is_dir();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_size = metadata.len();

            if is_dir {
                stack.push(path.clone());
            }

            if is_content {
                // Content search: only search regular files.
                if is_dir {
                    continue;
                }
                if file_size > MAX_CONTENT_FILE_SIZE {
                    continue;
                }
                if let Some((line_num, snippet)) = search_file_content(&path, matcher) {
                    total_found += 1;
                    if streamed < MAX_STREAMED_RESULTS {
                        let _ = channel.send(SearchEvent::Result(SearchResult {
                            path: path.to_string_lossy().to_string(),
                            name: file_name,
                            size: file_size,
                            is_dir: false,
                            line_number: Some(line_num),
                            snippet: Some(snippet),
                        }));
                        streamed += 1;
                    }
                }
            } else {
                // Name search: literal substring or regular expression.
                if matcher.is_match(&file_name) {
                    total_found += 1;
                    if streamed < MAX_STREAMED_RESULTS {
                        let _ = channel.send(SearchEvent::Result(SearchResult {
                            path: path.to_string_lossy().to_string(),
                            name: file_name,
                            size: file_size,
                            is_dir,
                            line_number: None,
                            snippet: None,
                        }));
                        streamed += 1;
                    }
                }
            }
        }
    }

    let _ = channel.send(SearchEvent::Done(SearchDone {
        total_found,
        cancelled: false,
    }));
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Search a file's content line-by-line and return the first match.
/// Returns the first match as (line_number, trimmed_snippet).
fn search_file_content(path: &PathBuf, matcher: &SearchMatcher) -> Option<(u32, String)> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    for (idx, line_result) in reader.lines().enumerate() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => return None, // binary / non-UTF-8 — skip entire file
        };
        if matcher.is_match(&line) {
            let trimmed = line.trim().to_string();
            // Cap snippet length for the frontend.
            let snippet = if trimmed.len() > 200 {
                format!("{}...", &trimmed[..200])
            } else {
                trimmed
            };
            return Some(((idx + 1) as u32, snippet));
        }
    }
    None
}
