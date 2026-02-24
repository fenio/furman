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
