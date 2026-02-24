use crate::models::FmError;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

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
        std::process::Command::new("open")
            .arg(&p)
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
