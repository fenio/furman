use crate::models::{FmError, VolumeInfo};
use nix::sys::statvfs::statvfs;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// List mounted volumes visible to the application.
#[tauri::command]
pub fn list_volumes() -> Result<Vec<VolumeInfo>, FmError> {
    list_volumes_impl()
}

/// Eject (macOS) or unmount (Linux) the volume at the given mount point.
/// Only call for volumes the backend marked as `ejectable`.
#[tauri::command]
pub fn eject_volume(mount_point: String) -> Result<(), FmError> {
    eject_volume_impl(&mount_point)
}

/// macOS: reads `/Volumes/` and always includes the root volume `/`.
#[cfg(target_os = "macos")]
fn list_volumes_impl() -> Result<Vec<VolumeInfo>, FmError> {
    let mut volumes: Vec<VolumeInfo> = Vec::new();

    // Always include the root volume.
    if let Ok(stat) = statvfs("/") {
        let block_size = stat.fragment_size();
        volumes.push(VolumeInfo {
            name: "Macintosh HD".to_string(),
            mount_point: "/".to_string(),
            total_space: block_size * stat.blocks() as u64,
            free_space: block_size * stat.blocks_available() as u64,
            fs_type: "apfs".to_string(),
            ejectable: false,
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
                    if *target == *"/" {
                        continue;
                    }
                }

                let name = entry.file_name().to_string_lossy().into_owned();

                let (total_space, free_space, fs_type) = if let Ok(stat) = statvfs(path.as_path()) {
                    let bs = stat.fragment_size();
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
                    // Anything under /Volumes/ on macOS is ejectable: external
                    // drives, disk images, and network mounts all live here.
                    ejectable: true,
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
        "proc",
        "sysfs",
        "tmpfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "pstore",
        "securityfs",
        "debugfs",
        "configfs",
        "fusectl",
        "mqueue",
        "hugetlbfs",
        "autofs",
        "efivarfs",
        "binfmt_misc",
        "tracefs",
        "bpf",
        "nsfs",
        "overlay",
        "squashfs",
    ];

    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(|e| FmError::Other(format!("Failed to read /proc/mounts: {e}")))?;

    let mut seen_mount_points = std::collections::HashSet::new();

    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let device = parts[0];
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
            (
                bs * stat.blocks() as u64,
                bs * stat.blocks_available() as u64,
            )
        } else {
            (0, 0)
        };

        let ejectable = mount_point != "/" && is_ejectable_linux(device, fs_type);

        volumes.push(VolumeInfo {
            name,
            mount_point: mount_point.to_string(),
            total_space,
            free_space,
            fs_type: fs_type.to_string(),
            ejectable,
        });
    }

    Ok(volumes)
}

/// Linux: decide whether a mount is "safe to eject". We treat as ejectable:
///   - Network filesystems (nfs/cifs/smb/sshfs/fuse.*/etc.) — always unmountable.
///   - Block devices whose backing disk is marked removable in sysfs.
#[cfg(target_os = "linux")]
fn is_ejectable_linux(device: &str, fs_type: &str) -> bool {
    // Network / FUSE-based filesystems
    const NETWORK_FS: &[&str] = &[
        "nfs",
        "nfs4",
        "cifs",
        "smb",
        "smbfs",
        "smb2",
        "smb3",
        "afp",
        "afpfs",
        "sshfs",
        "fuse.sshfs",
        "fuse.rclone",
        "fuse.s3fs",
        "fuse.gvfsd-fuse",
        "fuse",
        "fuseblk",
    ];
    if NETWORK_FS.contains(&fs_type) || fs_type.starts_with("fuse.") {
        return true;
    }

    // For block devices, walk /sys to find the parent disk and check `removable`.
    if let Some(dev_name) = device.strip_prefix("/dev/") {
        // Strip trailing digits (e.g. sdb1 -> sdb, nvme0n1p2 -> nvme0n1).
        let parent = parent_block_device(dev_name);
        let path = format!("/sys/block/{parent}/removable");
        if let Ok(s) = fs::read_to_string(&path) {
            return s.trim() == "1";
        }
    }
    false
}

/// Derive the parent block device name from a partition device name.
/// Examples: "sdb1" -> "sdb", "nvme0n1p2" -> "nvme0n1", "mmcblk0p1" -> "mmcblk0".
#[cfg(target_os = "linux")]
fn parent_block_device(dev: &str) -> String {
    // nvme/mmcblk use a 'p' separator before the partition number.
    if let Some(idx) = dev.rfind('p') {
        let (head, tail) = dev.split_at(idx);
        let after_p = &tail[1..];
        if !after_p.is_empty() && after_p.chars().all(|c| c.is_ascii_digit()) {
            // Only strip if what precedes 'p' ends in a digit (nvme0n1, mmcblk0).
            if head.chars().next_back().is_some_and(|c| c.is_ascii_digit()) {
                return head.to_string();
            }
        }
    }
    // Standard sdX/hdX: strip trailing digits.
    let trimmed = dev.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        dev.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(target_os = "macos")]
fn eject_volume_impl(mount_point: &str) -> Result<(), FmError> {
    if mount_point == "/" {
        return Err(FmError::Other("Cannot eject the root volume".to_string()));
    }
    let out = Command::new("diskutil")
        .arg("eject")
        .arg(mount_point)
        .output()
        .map_err(|e| FmError::Other(format!("Failed to run diskutil: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(FmError::Other(format!("Eject failed: {msg}")))
    }
}

#[cfg(target_os = "linux")]
fn eject_volume_impl(mount_point: &str) -> Result<(), FmError> {
    if mount_point == "/" {
        return Err(FmError::Other("Cannot unmount the root volume".to_string()));
    }
    // Prefer udisksctl (no root needed for user-mounted removables).
    if let Ok(out) = Command::new("udisksctl")
        .arg("unmount")
        .arg("--no-user-interaction")
        .arg("--mount-point")
        .arg(mount_point)
        .output()
    {
        if out.status.success() {
            return Ok(());
        }
    }
    // Fallback to plain umount.
    let out = Command::new("umount")
        .arg(mount_point)
        .output()
        .map_err(|e| FmError::Other(format!("Failed to run umount: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        Err(FmError::Other(format!("Unmount failed: {msg}")))
    }
}
