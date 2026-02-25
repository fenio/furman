use crate::models::{DirListing, FileEntry, FmError};
use nix::sys::statvfs::statvfs;
use nix::unistd::{Gid, Group, Uid, User};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

// ── Git status helpers ──────────────────────────────────────────────────────

fn porcelain_to_status(x: u8, y: u8) -> char {
    // Conflicts
    if x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D') {
        return 'U';
    }
    // Ignored
    if x == b'!' {
        return '!';
    }
    // Untracked
    if x == b'?' {
        return '?';
    }
    // Worktree changes take visual priority
    if y == b'M' || y == b'D' {
        return 'M';
    }
    // Index-only changes
    match x {
        b'R' => 'R',
        b'M' => 'M',
        b'A' => 'A',
        b'D' => 'D',
        _ => '?',
    }
}

fn higher_priority(a: char, b: char) -> char {
    fn rank(c: char) -> u8 {
        match c {
            'U' => 6,
            'M' => 5,
            'A' => 4,
            'D' => 3,
            'R' => 2,
            '?' => 1,
            '!' => 0,
            _ => 0,
        }
    }
    if rank(a) >= rank(b) { a } else { b }
}

fn get_git_statuses(dir_path: &Path) -> HashMap<String, char> {
    let mut map = HashMap::new();

    // Find repo root
    let root_out = match Command::new("git")
        .args(["-C", &dir_path.to_string_lossy(), "rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return map,
    };

    let repo_root = PathBuf::from(String::from_utf8_lossy(&root_out.stdout).trim());

    // Compute the directory path relative to repo root
    let dir_canonical = dir_path.canonicalize().unwrap_or_else(|_| dir_path.to_path_buf());
    let root_canonical = repo_root.canonicalize().unwrap_or(repo_root.clone());
    let dir_rel = dir_canonical
        .strip_prefix(&root_canonical)
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .into_owned();

    // Run git status
    let status_out = match Command::new("git")
        .args([
            "-C",
            &root_canonical.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-unormal",
            "--ignored",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return map,
    };

    let output = String::from_utf8_lossy(&status_out.stdout);

    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let x = line.as_bytes()[0];
        let y = line.as_bytes()[1];
        let file_path_str = &line[3..];

        // For renames, use the destination path (after " -> ")
        let effective_path = if x == b'R' {
            file_path_str
                .split(" -> ")
                .last()
                .unwrap_or(file_path_str)
        } else {
            file_path_str
        };

        // Strip trailing slash from directories
        let effective_path = effective_path.trim_end_matches('/');

        // Check if this path is under our listed directory
        let entry_name = if dir_rel.is_empty() {
            // We're at the repo root
            effective_path
        } else {
            let prefix = format!("{}/", dir_rel);
            match effective_path.strip_prefix(&prefix) {
                Some(rest) => rest,
                None => continue,
            }
        };

        // First component = the entry name in our listing
        let name = match entry_name.split('/').next() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let status = porcelain_to_status(x, y);

        map.entry(name)
            .and_modify(|existing| *existing = higher_priority(*existing, status))
            .or_insert(status);
    }

    map
}

/// Build a `FileEntry` from a directory entry.
fn entry_from_path(path: &Path) -> Result<FileEntry, FmError> {
    // Use symlink_metadata so we can detect symlinks.
    let sym_meta = fs::symlink_metadata(path)?;
    let is_symlink = sym_meta.file_type().is_symlink();

    // For the "real" metadata (size, is_dir, etc.) follow the link.
    let meta = if is_symlink {
        fs::metadata(path).unwrap_or_else(|_| sym_meta.clone())
    } else {
        sym_meta.clone()
    };

    let symlink_target = if is_symlink {
        fs::read_link(path)
            .ok()
            .map(|t| t.to_string_lossy().into_owned())
    } else {
        None
    };

    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let permissions = sym_meta.permissions().mode();

    // Owner / group names via nix (safe on Apple Silicon, unlike `users` crate).
    let uid = sym_meta.uid();
    let gid = sym_meta.gid();

    let owner = User::from_uid(Uid::from_raw(uid))
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| uid.to_string());

    let group = Group::from_gid(Gid::from_raw(gid))
        .ok()
        .flatten()
        .map(|g| g.name)
        .unwrap_or_else(|| gid.to_string());

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned());

    Ok(FileEntry {
        name,
        path: path.to_string_lossy().into_owned(),
        size: meta.len(),
        is_dir: meta.is_dir(),
        is_symlink,
        symlink_target,
        modified,
        permissions,
        owner,
        group,
        extension,
        git_status: None,
        storage_class: None,
    })
}

/// List the contents of a directory.
///
/// If `path` is empty the user's home directory is used.
/// Entries are sorted directories-first, then alphabetically (case-insensitive).
#[tauri::command]
pub fn list_directory(path: String, show_hidden: bool) -> Result<DirListing, FmError> {
    let dir: PathBuf = if path.is_empty() {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    } else {
        PathBuf::from(&path)
    };

    if !dir.exists() {
        return Err(FmError::NotFound(dir.to_string_lossy().into_owned()));
    }
    if !dir.is_dir() {
        return Err(FmError::Other(format!(
            "{} is not a directory",
            dir.display()
        )));
    }

    let mut entries: Vec<FileEntry> = Vec::new();
    let mut total_size: u64 = 0;

    let git_statuses = get_git_statuses(&dir);

    // Prepend a ".." entry for parent navigation (unless we are at the root).
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    if canonical != PathBuf::from("/") {
        if let Some(parent) = canonical.parent() {
            entries.push(FileEntry {
                name: "..".to_string(),
                path: parent.to_string_lossy().into_owned(),
                size: 0,
                is_dir: true,
                is_symlink: false,
                symlink_target: None,
                modified: 0,
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                extension: None,
                git_status: None,
                storage_class: None,
            });
        }
    }

    for item in fs::read_dir(&dir)? {
        let item = item?;
        let file_name = item.file_name().to_string_lossy().into_owned();

        // Skip hidden files unless requested.
        if !show_hidden && file_name.starts_with('.') {
            continue;
        }

        match entry_from_path(&item.path()) {
            Ok(mut entry) => {
                entry.git_status = git_statuses.get(&entry.name).map(|c| c.to_string());
                total_size += entry.size;
                entries.push(entry);
            }
            Err(_) => {
                // Skip entries we cannot stat (e.g. broken symlinks we already
                // handle above, but there may be other exotic failures).
                continue;
            }
        }
    }

    // Sort: directories first, then alphabetical case-insensitive.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    // Free space via statvfs.
    let free_space = statvfs(&dir)
        .map(|s| s.fragment_size() as u64 * s.blocks_available() as u64)
        .unwrap_or(0);

    Ok(DirListing {
        path: dir.to_string_lossy().into_owned(),
        entries,
        total_size,
        free_space,
    })
}

/// Create a new directory (including intermediate parents).
#[tauri::command]
pub fn create_directory(path: String) -> Result<(), FmError> {
    let p = PathBuf::from(&path);
    if p.exists() {
        return Err(FmError::AlreadyExists(path));
    }
    fs::create_dir_all(&p)?;
    Ok(())
}

/// Recursively calculate the total size of a directory.
#[tauri::command]
pub fn get_directory_size(path: String) -> Result<u64, FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }
    Ok(dir_size_recursive(&p))
}

fn dir_size_recursive(path: &Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                total += dir_size_recursive(&entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}
