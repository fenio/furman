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

/// Mount an SMB or NFS network share via the operating system and return the
/// local mount point so the caller can navigate to it as a normal folder.
///
/// This deliberately delegates to the OS rather than speaking SMB/NFS natively:
///   - macOS: `open <url>` hands the URL to NetFS, which mounts under `/Volumes`
///     and reuses the system Keychain. The new mount appears in `list_volumes`
///     and is ejectable, so the rest of the app needs no changes.
///   - Linux: `gio mount` (GVfs) mounts the share unprivileged under the user's
///     gvfs runtime directory. NFS is not supported there (it needs root).
///
/// `protocol` is "smb" or "nfs". `password`/`username`/`domain` are optional;
/// when omitted the OS may prompt (macOS) or fall back to guest access.
#[tauri::command]
pub async fn mount_network_share(
    protocol: String,
    host: String,
    share: String,
    username: Option<String>,
    password: Option<String>,
    domain: Option<String>,
) -> Result<String, FmError> {
    tauri::async_runtime::spawn_blocking(move || {
        mount_network_share_impl(
            &protocol,
            &host,
            &share,
            username.as_deref(),
            password.as_deref(),
            domain.as_deref(),
        )
    })
    .await
    .map_err(|e| FmError::Other(format!("Mount task failed: {e}")))?
}

/// Percent-encode a userinfo component (username/password) for use in a URL.
/// Encodes everything outside the RFC 3986 "unreserved" set so credentials
/// containing `@`, `:`, `/`, spaces, etc. survive being placed in the URL.
fn encode_userinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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

// ── Network share mounting ───────────────────────────────────────────────────

/// macOS: build an `smb://` / `nfs://` URL and let NetFS mount it via `open`.
/// We can't know the exact `/Volumes/<name>` NetFS will choose (it disambiguates
/// duplicates with a numeric suffix), so we snapshot `/Volumes` before and after
/// and return whatever new entry appears.
#[cfg(target_os = "macos")]
fn mount_network_share_impl(
    protocol: &str,
    host: &str,
    share: &str,
    username: Option<&str>,
    password: Option<&str>,
    _domain: Option<&str>,
) -> Result<String, FmError> {
    let scheme = match protocol {
        "smb" => "smb",
        "nfs" => "nfs",
        other => return Err(FmError::Other(format!("Unsupported protocol: {other}"))),
    };

    // NFS has no in-URL auth; SMB carries optional credentials in the userinfo.
    let userinfo = if scheme == "smb" {
        match (username, password) {
            (Some(u), Some(p)) => format!("{}:{}@", encode_userinfo(u), encode_userinfo(p)),
            (Some(u), None) => format!("{}@", encode_userinfo(u)),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let share = share.trim_start_matches('/');
    let url = format!("{scheme}://{userinfo}{host}/{share}");

    let before = volume_names();

    // `-g` keeps Finder from being brought to the foreground.
    let out = Command::new("open")
        .arg("-g")
        .arg(&url)
        .output()
        .map_err(|e| FmError::Other(format!("Failed to run open: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(FmError::Other(format!(
            "Failed to start mount: {}",
            stderr.trim()
        )));
    }

    // Poll for the newly mounted volume (up to ~20s). If the share needs
    // credentials we didn't supply, NetFS shows its own auth dialog and this
    // will time out — the message points the user there.
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let now = volume_names();
        if let Some(new_mount) = now.difference(&before).next() {
            return Ok(new_mount.clone());
        }
    }
    Err(FmError::Other(
        "Timed out waiting for the share to mount. It may require authentication — check Finder."
            .to_string(),
    ))
}

/// macOS: snapshot the set of mount points currently under `/Volumes`.
#[cfg(target_os = "macos")]
fn volume_names() -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Ok(rd) = fs::read_dir("/Volumes") {
        for entry in rd.flatten() {
            set.insert(entry.path().to_string_lossy().into_owned());
        }
    }
    set
}

/// Linux: mount SMB shares unprivileged through GVfs (`gio mount`). The username
/// and domain are placed in the URL so only the password needs to be fed on
/// stdin, then we locate the resulting entry under the gvfs runtime directory.
/// NFS isn't supported here (it requires a privileged `mount -t nfs`).
#[cfg(target_os = "linux")]
fn mount_network_share_impl(
    protocol: &str,
    host: &str,
    share: &str,
    username: Option<&str>,
    password: Option<&str>,
    domain: Option<&str>,
) -> Result<String, FmError> {
    use std::io::Write;
    use std::process::Stdio;

    match protocol {
        "smb" => {}
        "nfs" => {
            return Err(FmError::Other(
                "NFS mounting on Linux requires root. Mount it manually \
                 (e.g. `sudo mount -t nfs host:/share /mnt/...`), then browse it as a local folder."
                    .to_string(),
            ));
        }
        other => return Err(FmError::Other(format!("Unsupported protocol: {other}"))),
    }

    if Command::new("gio").arg("--help").output().is_err() {
        return Err(FmError::Other(
            "`gio` is not available. Install it (glib2/gvfs) or mount the share manually."
                .to_string(),
        ));
    }

    let share = share.trim_start_matches('/');
    // gvfs URI: smb://[domain;][user@]host/share
    let mut userpart = String::new();
    if let Some(d) = domain.filter(|d| !d.is_empty()) {
        userpart.push_str(&format!("{d};"));
    }
    if let Some(u) = username.filter(|u| !u.is_empty()) {
        userpart.push_str(&format!("{u}@"));
    }
    let url = format!("smb://{userpart}{host}/{share}");

    let mut child = Command::new("gio")
        .arg("mount")
        .arg(&url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| FmError::Other(format!("Failed to run gio: {e}")))?;

    // `gio mount` prompts for the password on stdin (user/domain are in the URL).
    // Feed the password (or a blank line to accept guest/anonymous access).
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{}", password.unwrap_or(""));
    }
    let out = child
        .wait_with_output()
        .map_err(|e| FmError::Other(format!("gio mount failed: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(FmError::Other(format!(
            "Failed to mount share: {}",
            stderr.trim()
        )));
    }

    // Locate the gvfs entry, e.g.
    //   /run/user/<uid>/gvfs/smb-share:server=host,share=share[,user=...]
    let uid = nix::unistd::getuid().as_raw();
    let gvfs = format!("/run/user/{uid}/gvfs");
    let host_l = host.to_lowercase();
    let needle_server = format!("server={host_l}");
    let needle_share = format!("share={}", share.to_lowercase());
    if let Ok(rd) = fs::read_dir(&gvfs) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("smb-share:")
                && name.contains(&needle_server)
                && name.contains(&needle_share)
            {
                return Ok(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    Err(FmError::Other(format!(
        "Share mounted but its location under {gvfs} could not be found."
    )))
}

/// Fallback for platforms without an in-app mount path (e.g. Windows for now).
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn mount_network_share_impl(
    _protocol: &str,
    _host: &str,
    _share: &str,
    _username: Option<&str>,
    _password: Option<&str>,
    _domain: Option<&str>,
) -> Result<String, FmError> {
    Err(FmError::Other(
        "Mounting network shares from the app is not supported on this platform yet.".to_string(),
    ))
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
