use crate::models::FmError;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

fn validate_category(category: &str) -> Result<(), FmError> {
    if category.is_empty()
        || !category
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(FmError::Other("Invalid temp category".into()));
    }
    Ok(())
}

pub fn create_temp_dir_path(category: &str) -> Result<PathBuf, FmError> {
    validate_category(category)?;
    for _ in 0..10 {
        let path = std::env::temp_dir().join(format!("furman-{category}-{}", uuid::Uuid::new_v4()));
        let result = {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(&path)
        };
        match result {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(FmError::Io(error)),
        }
    }
    Err(FmError::Other(
        "Failed to allocate unique temp directory".into(),
    ))
}

pub fn safe_filename(name: &str) -> String {
    let filename = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    if filename.is_empty() || filename == "." || filename == ".." {
        "file".to_string()
    } else {
        filename.to_string()
    }
}

pub fn safe_relative_path(path: &str) -> Result<PathBuf, FmError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.to_string_lossy().starts_with(['-', '@'])
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(FmError::Other("Invalid archive path".into()));
    }
    Ok(path.to_path_buf())
}

fn allocation_root(path: &Path) -> Result<PathBuf, FmError> {
    let platform_root = std::env::temp_dir();
    let root = std::fs::canonicalize(&platform_root).unwrap_or_else(|_| platform_root.clone());
    let normalized_path = match path.strip_prefix(&platform_root) {
        Ok(relative) => root.join(relative),
        Err(_) => path.to_path_buf(),
    };
    let relative = normalized_path
        .strip_prefix(&root)
        .map_err(|_| FmError::Other("Refusing to clean path outside Furman temp root".into()))?;
    let mut components = relative.components();
    let allocation_name = match components.next() {
        Some(Component::Normal(value)) => value.to_string_lossy(),
        _ => return Err(FmError::Other("Invalid Furman temp path".into())),
    };
    let value = allocation_name
        .strip_prefix("furman-")
        .ok_or_else(|| FmError::Other("Invalid Furman temp allocation".into()))?;
    if value.len() < 38 {
        return Err(FmError::Other("Invalid Furman temp allocation".into()));
    }
    let category = value
        .get(..value.len() - 37)
        .ok_or_else(|| FmError::Other("Invalid Furman temp allocation".into()))?;
    validate_category(category)?;
    let separator = value
        .get(value.len() - 37..value.len() - 36)
        .ok_or_else(|| FmError::Other("Invalid Furman temp allocation".into()))?;
    if separator != "-" {
        return Err(FmError::Other("Invalid Furman temp allocation".into()));
    }
    let id = value
        .get(value.len() - 36..)
        .ok_or_else(|| FmError::Other("Invalid Furman temp allocation".into()))?;
    uuid::Uuid::parse_str(id)
        .map_err(|_| FmError::Other("Invalid Furman temp allocation".into()))?;
    Ok(root.join(allocation_name.as_ref()))
}

#[tauri::command]
pub fn create_temp_dir(category: String) -> Result<String, FmError> {
    Ok(create_temp_dir_path(&category)?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub fn cleanup_temp_path(path: String) -> Result<(), FmError> {
    let allocation = allocation_root(Path::new(&path))?;
    match std::fs::remove_dir_all(allocation) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FmError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_are_unique_and_cleanup_is_recursive() {
        let first = create_temp_dir_path("test").unwrap();
        let second = create_temp_dir_path("test").unwrap();
        let canonical_second = std::fs::canonicalize(&second).unwrap();
        assert_ne!(first, second);
        std::fs::create_dir_all(first.join("nested")).unwrap();
        std::fs::write(first.join("nested/file.txt"), b"data").unwrap();

        cleanup_temp_path(first.join("nested/file.txt").to_string_lossy().into_owned()).unwrap();
        assert!(!first.exists());
        cleanup_temp_path(first.to_string_lossy().into_owned()).unwrap();
        cleanup_temp_path(canonical_second.to_string_lossy().into_owned()).unwrap();
    }

    #[test]
    fn cleanup_rejects_paths_outside_temp_root() {
        let result = cleanup_temp_path(std::env::temp_dir().to_string_lossy().into_owned());
        assert!(result.is_err());
    }

    #[test]
    fn relative_archive_paths_reject_traversal() {
        assert!(safe_relative_path("folder/file.txt").is_ok());
        assert!(safe_relative_path("../file.txt").is_err());
        assert!(safe_relative_path("/file.txt").is_err());
        assert!(safe_relative_path("-spf").is_err());
        assert!(safe_relative_path("@files.txt").is_err());
    }
}
