use crate::models::{FmError, VolumeInfo};
use nix::sys::statvfs::statvfs;
use std::fs;
use std::path::PathBuf;

/// List mounted volumes visible to the application.
#[tauri::command]
pub fn list_volumes() -> Result<Vec<VolumeInfo>, FmError> {
    list_volumes_impl()
}

/// macOS: reads `/Volumes/` and always includes the root volume `/`.
#[cfg(target_os = "macos")]
fn list_volumes_impl() -> Result<Vec<VolumeInfo>, FmError> {
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

/// Linux: parses `/proc/mounts` and filters to real filesystems.
#[cfg(target_os = "linux")]
fn list_volumes_impl() -> Result<Vec<VolumeInfo>, FmError> {
    let mut volumes: Vec<VolumeInfo> = Vec::new();

    // Virtual/pseudo filesystem types to skip
    const SKIP_FS: &[&str] = &[
        "proc", "sysfs", "tmpfs", "devtmpfs", "devpts", "cgroup", "cgroup2",
        "pstore", "securityfs", "debugfs", "configfs", "fusectl", "mqueue",
        "hugetlbfs", "autofs", "efivarfs", "binfmt_misc", "tracefs",
        "bpf", "nsfs", "overlay", "squashfs",
    ];

    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(|e| FmError::Other(format!("Failed to read /proc/mounts: {e}")))?;

    let mut seen_mount_points = std::collections::HashSet::new();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let _device = parts[0];
        let mount_point = parts[1];
        let fs_type = parts[2];

        if SKIP_FS.contains(&fs_type) {
            continue;
        }

        // Skip snap mounts
        if mount_point.starts_with("/snap/") {
            continue;
        }

        // Skip duplicate mount points
        if !seen_mount_points.insert(mount_point.to_string()) {
            continue;
        }

        let name = if mount_point == "/" {
            "Root".to_string()
        } else {
            PathBuf::from(mount_point)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| mount_point.to_string())
        };

        let (total_space, free_space) = if let Ok(stat) = statvfs(mount_point) {
            let bs = stat.fragment_size() as u64;
            (bs * stat.blocks() as u64, bs * stat.blocks_available() as u64)
        } else {
            (0, 0)
        };

        volumes.push(VolumeInfo {
            name,
            mount_point: mount_point.to_string(),
            total_space,
            free_space,
            fs_type: fs_type.to_string(),
        });
    }

    Ok(volumes)
}
