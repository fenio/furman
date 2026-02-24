use crate::models::{FmError, VolumeInfo};
use nix::sys::statvfs::statvfs;
use std::fs;
use std::path::PathBuf;

/// List mounted volumes visible to the application.
///
/// On macOS this reads `/Volumes/` and always includes the root volume `/`.
/// Each entry is enriched with capacity / free-space information from
/// `statvfs`.
#[tauri::command]
pub fn list_volumes() -> Result<Vec<VolumeInfo>, FmError> {
    let mut volumes: Vec<VolumeInfo> = Vec::new();

    // Always include the root volume.
    if let Ok(stat) = statvfs("/") {
        let block_size = stat.fragment_size() as u64;
        volumes.push(VolumeInfo {
            name: "Macintosh HD".to_string(),
            mount_point: "/".to_string(),
            total_space: block_size * stat.blocks() as u64,
            free_space: block_size * stat.blocks_available() as u64,
            fs_type: "apfs".to_string(),
        });
    }

    // Enumerate /Volumes/.
    let volumes_dir = PathBuf::from("/Volumes");
    if volumes_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&volumes_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                let mount_point = path.to_string_lossy().into_owned();

                // Skip if this is just a symlink to "/" (the boot volume alias).
                if let Ok(target) = fs::read_link(&path) {
                    if target == PathBuf::from("/") {
                        continue;
                    }
                }

                let name = entry.file_name().to_string_lossy().into_owned();

                let (total_space, free_space, fs_type) =
                    if let Ok(stat) = statvfs(path.as_path()) {
                        let bs = stat.fragment_size() as u64;
                        (
                            bs * stat.blocks() as u64,
                            bs * stat.blocks_available() as u64,
                            String::new(), // fs type not easily available via statvfs
                        )
                    } else {
                        (0, 0, String::new())
                    };

                volumes.push(VolumeInfo {
                    name,
                    mount_point,
                    total_space,
                    free_space,
                    fs_type,
                });
            }
        }
    }

    Ok(volumes)
}
