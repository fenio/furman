use crate::models::{FileProperties, FmError};
use nix::unistd::{Gid, Group, Uid, User};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

/// Read an entire file as UTF-8 text.
#[tauri::command]
pub fn read_file_text(path: String) -> Result<String, FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }
    Ok(fs::read_to_string(&p)?)
}

/// Write UTF-8 text to a file, creating it if necessary and truncating if it
/// already exists.
#[tauri::command]
pub fn write_file_text(path: String, content: String) -> Result<(), FmError> {
    let p = PathBuf::from(&path);
    // Ensure parent directories exist.
    if let Some(parent) = p.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&p, content)?;
    Ok(())
}

/// Read a chunk of raw bytes from a file starting at `offset` for `length`
/// bytes.  Returns the bytes as a `Vec<u8>` which Tauri serialises as a
/// JSON array of numbers.
#[tauri::command]
pub fn read_file_binary(path: String, offset: u64, length: u64) -> Result<Vec<u8>, FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }

    let mut file = fs::File::open(&p)?;
    file.seek(SeekFrom::Start(offset))?;

    let mut buf = vec![0u8; length as usize];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Open a file with the system's default application.
#[tauri::command]
pub fn open_file_default(path: String) -> Result<(), FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }
    #[cfg(target_os = "macos")]
    {
        // Use "open -a Finder" for directories whose names end in ".app",
        // because plain "open" treats them as application bundles.
        let mut cmd = std::process::Command::new("open");
        if p.is_dir()
            && p.extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("app"))
        {
            cmd.args(["-a", "Finder"]);
        }
        cmd.arg(&p)
            .spawn()
            .map_err(|e| FmError::Other(format!("Failed to open file: {e}")))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&p)
            .spawn()
            .map_err(|e| FmError::Other(format!("Failed to open file: {e}")))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &p.to_string_lossy()])
            .spawn()
            .map_err(|e| FmError::Other(format!("Failed to open file: {e}")))?;
    }
    Ok(())
}

/// Open a file in the user's configured external editor.
///
/// Terminal-based editors (vim, nvim, vi, nano, emacs, helix, hx, micro) are
/// wrapped via `osascript` so they open in a new Terminal.app window on macOS.
/// GUI editors (code, subl, zed, etc.) are spawned directly.
#[tauri::command]
pub fn open_in_editor(path: String, editor: String) -> Result<(), FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }

    let parts: Vec<&str> = editor.split_whitespace().collect();
    if parts.is_empty() {
        return Err(FmError::Other("Editor command is empty".into()));
    }

    let exe = parts[0];
    let extra_args = &parts[1..];

    const TERMINAL_EDITORS: &[&str] = &[
        "vim", "nvim", "vi", "nano", "emacs", "helix", "hx", "micro",
    ];

    let exe_basename = std::path::Path::new(exe)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(exe);

    if TERMINAL_EDITORS.contains(&exe_basename) {
        // Wrap in a new Terminal.app window via osascript
        let mut cmd_parts: Vec<String> = vec![exe.to_string()];
        cmd_parts.extend(extra_args.iter().map(|s| s.to_string()));
        cmd_parts.push(p.to_string_lossy().into_owned());

        // Shell-escape each part for the AppleScript string
        let escaped: Vec<String> = cmd_parts
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "'\\''")))
            .collect();
        let shell_cmd = escaped.join(" ");

        let script = format!(
            "tell application \"Terminal\"\n\
                activate\n\
                do script \"{}\"\n\
            end tell",
            shell_cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );

        std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| FmError::Other(format!("Failed to open terminal editor: {e}")))?;
    } else {
        // GUI editor — spawn directly
        let mut cmd = std::process::Command::new(exe);
        cmd.args(extra_args);
        cmd.arg(&p);
        cmd.spawn()
            .map_err(|e| FmError::Other(format!("Failed to open editor: {e}")))?;
    }

    Ok(())
}

/// Set the Unix permission mode of a file or directory (chmod).
///
/// Uses `std::fs::set_permissions` with `PermissionsExt::from_mode()` which
/// maps directly to `chmod(2)` on Unix.
#[tauri::command]
pub fn set_permissions(path: String, mode: u32) -> Result<(), FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }

    let perms = fs::Permissions::from_mode(mode);
    fs::set_permissions(&p, perms)?;

    Ok(())
}

/// Return the path to the application log directory.
#[tauri::command]
pub fn get_log_path() -> Result<String, FmError> {
    let dir = dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Logs/com.furman.app");
    Ok(dir.to_string_lossy().into_owned())
}

/// Get rich file/directory properties including birth time, access time, etc.
#[tauri::command]
pub fn get_file_properties(path: String) -> Result<FileProperties, FmError> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(FmError::NotFound(path));
    }

    let sym_meta = fs::symlink_metadata(&p)?;
    let is_symlink = sym_meta.file_type().is_symlink();

    let meta = if is_symlink {
        fs::metadata(&p).unwrap_or_else(|_| sym_meta.clone())
    } else {
        sym_meta.clone()
    };

    let symlink_target = if is_symlink {
        fs::read_link(&p)
            .ok()
            .map(|t| t.to_string_lossy().into_owned())
    } else {
        None
    };

    let kind = if is_symlink {
        "Symlink".to_string()
    } else if meta.is_dir() {
        "Directory".to_string()
    } else {
        "File".to_string()
    };

    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let accessed = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let permissions = sym_meta.permissions().mode();

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

    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    Ok(FileProperties {
        name,
        path: p.to_string_lossy().into_owned(),
        size: meta.len(),
        is_dir: meta.is_dir(),
        is_symlink,
        symlink_target,
        created,
        modified,
        accessed,
        permissions,
        owner,
        group,
        kind,
    })
}
