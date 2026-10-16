//! Bounded, read-only 7z subprocesses. Never extract archive paths to disk.
use super::{is_excluded, FileInfo};
use crate::models::FmError;
use crate::s3::scan::{ScanBudget, ScanProgress};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const LIST_LIMIT: usize = 16 * 1024 * 1024;
const STDERR_LIMIT: usize = 8192;

fn error(message: impl Into<String>) -> FmError {
    FmError::Other(message.into())
}

fn safe_member(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('@')
        && !path.starts_with('/')
        && !path.contains(['\\', '\0', '\r', '\n', '#'])
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
}

fn parse_uri(uri: &str) -> Result<(&str, String), FmError> {
    let (archive, member) = uri
        .strip_prefix("archive://")
        .and_then(|s| s.rsplit_once('#'))
        .ok_or_else(|| error("Invalid archive comparison location"))?;
    if !Path::new(archive).is_absolute()
        || archive.contains('\0')
        || (!member.is_empty() && !safe_member(member.trim_end_matches('/')))
    {
        return Err(error(
            "Archive comparison requires an absolute archive and a safe internal folder",
        ));
    }
    Ok((
        archive,
        if member.is_empty() {
            String::new()
        } else {
            format!("{}/", member.trim_end_matches('/'))
        },
    ))
}

#[derive(Debug)]
struct Member {
    path: String,
    size: u64,
    modified: i64,
    directory: bool,
}

fn parse_listing(text: &str) -> Result<Vec<Member>, FmError> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for block in text.split("\n\n").filter(|block| !block.trim().is_empty()) {
        let mut fields = HashMap::new();
        for line in block.lines() {
            let (key, value) = line
                .split_once(" = ")
                .ok_or_else(|| error("Ambiguous 7z comparison listing"))?;
            if fields.insert(key, value).is_some() {
                return Err(error("Duplicate 7z listing attribute"));
            }
        }
        let path = *fields
            .get("Path")
            .ok_or_else(|| error("7z did not return member names"))?;
        if fields.get("Encrypted") == Some(&"+") {
            return Err(error("Encrypted archive members are not supported by comparison; no password prompt was opened"));
        }
        let attributes = fields.get("Attributes").copied().unwrap_or("");
        let mode = fields.get("Mode").copied().unwrap_or("");
        let directory = fields.get("Folder") == Some(&"+")
            || attributes.starts_with('D')
            || attributes
                .split_whitespace()
                .any(|part| part.starts_with('d'))
            || mode.starts_with('d');
        let path = if directory {
            path.trim_end_matches('/')
        } else {
            path
        };
        if !safe_member(path) || !seen.insert(path.to_owned()) {
            return Err(error(
                "Unsafe, duplicate or unrepresentable archive member path",
            ));
        }
        if fields
            .get("Symbolic Link")
            .is_some_and(|value| !value.is_empty())
            || fields
                .get("Hard Link")
                .is_some_and(|value| !value.is_empty())
            || attributes
                .split_whitespace()
                .any(|part| part.starts_with(['l', 'b', 'c', 'p', 's']))
            || mode.starts_with(['l', 'b', 'c', 'p', 's'])
        {
            return Err(error("Archive comparison refuses link/special entries; compare regular-file archives only"));
        }
        let size = fields
            .get("Size")
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| error("7z did not return a valid member size"))?;
        let date = fields.get("Modified").copied().unwrap_or("");
        let modified = if date.is_empty() {
            0
        } else {
            chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S%.f")
                .map_err(|_| error("Unrecognized archive modification time"))?
                .and_utc()
                .timestamp_millis()
        };
        result.push(Member {
            path: path.to_owned(),
            size,
            modified,
            directory,
        });
    }
    // A file named "folder" alongside "folder/child" cannot be mapped safely.
    let files: HashSet<_> = result
        .iter()
        .filter(|m| !m.directory)
        .map(|m| m.path.as_str())
        .collect();
    for member in &result {
        for (index, _) in member.path.match_indices('/') {
            if files.contains(&member.path[..index]) {
                return Err(error("Archive contains a file/directory path collision"));
            }
        }
    }
    Ok(result)
}

async fn read_capped(reader: impl AsyncRead + Unpin, maximum: usize) -> Result<Vec<u8>, FmError> {
    let mut bytes = Vec::new();
    reader
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > maximum {
        return Err(FmError::ScanLimited("Archive subprocess output exceeded its comparison limit (16 MiB listing / 8 KiB diagnostics)".into()));
    }
    Ok(bytes)
}

fn command(args: &[&str]) -> Command {
    let mut command = Command::new("7z");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn listing(
    archive: &str,
    cancel: &AtomicBool,
    budget: &ScanBudget,
) -> Result<Vec<Member>, FmError> {
    budget.wait(cancel, async {
        let mut child = command(&["l", "-slt", "-ba", "-sccUTF-8", "--", archive]).spawn()
            .map_err(|e| error(format!("Could not start 7z for read-only comparison (install 7zip if missing): {e}")))?;
        let stdout = child.stdout.take().ok_or_else(|| error("Missing 7z stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| error("Missing 7z stderr"))?;
        let (bytes, diagnostic) = tokio::try_join!(read_capped(stdout, LIST_LIMIT), read_capped(stderr, STDERR_LIMIT))?;
        if !child.wait().await?.success() { return Err(error(format!("7z could not list this archive. It may be encrypted, unsupported or damaged; no files were extracted.\n{}", String::from_utf8_lossy(&diagnostic)))); }
        let text = String::from_utf8(bytes).map_err(|_| error("Archive member names are not valid UTF-8"))?;
        parse_listing(&text.replace("\r\n", "\n"))
    }).await
}

async fn hash_member(
    archive: &str,
    member: &Member,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<String, FmError> {
    budget.wait(cancel, async {
        // -spd disables wildcard expansion: the member name is literal. -so
        // streams contents only; no paths, links or archive data reach the disk.
        let mut child = command(&["x", "-so", "-spd", "-y", "--", archive, &member.path]).spawn()?;
        let mut stdout = child.stdout.take().ok_or_else(|| error("Missing 7z stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| error("Missing 7z stderr"))?;
        let stream = async {
            let mut hasher = Sha256::new(); let mut total = 0u64; let mut buffer = [0u8; 64 * 1024];
            loop {
                budget.check(cancel)?;
                let count = stdout.read(&mut buffer).await?;
                if count == 0 { break; }
                total = total.checked_add(count as u64).ok_or_else(|| error("Archive member size overflow"))?;
                if total > member.size { return Err(error("Archive member expanded beyond its listed size")); }
                hasher.update(&buffer[..count]); budget.bytes(count as u64, false);
                if budget.progress().bytes_read % (1024 * 1024) < count as u64 { progress(budget.progress()); }
            }
            if total != member.size { return Err(error("Archive member stream was incomplete")); }
            Ok::<_, FmError>(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>())
        };
        let (hash, diagnostic) = tokio::try_join!(stream, read_capped(stderr, STDERR_LIMIT))?;
        if !child.wait().await?.success() { return Err(error(format!("7z could not verify this member (encryption, corruption or unsupported format); comparison is incomplete\n{}", String::from_utf8_lossy(&diagnostic)))); }
        Ok(hash)
    }).await
}

pub(super) async fn collect(
    uri: &str,
    checksum: bool,
    cancel: &AtomicBool,
    excludes: &[String],
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<HashMap<String, FileInfo>, FmError> {
    let (archive, prefix) = parse_uri(uri)?;
    let before = tokio::fs::symlink_metadata(archive).await?;
    if !before.is_file() {
        return Err(error(
            "Archive comparison requires a regular, non-symlink archive file",
        ));
    }
    let members = listing(archive, cancel, budget).await?;
    let folder_exists = prefix.is_empty()
        || members.iter().any(|m| {
            m.path == prefix.trim_end_matches('/') && m.directory || m.path.starts_with(&prefix)
        });
    if !folder_exists {
        return Err(error("Archive comparison folder no longer exists"));
    }
    let mut files = HashMap::new();
    for member in members {
        budget.object(cancel)?;
        if member.directory {
            continue;
        }
        let Some(relative) = member.path.strip_prefix(&prefix) else {
            continue;
        };
        if is_excluded(relative, excludes) {
            continue;
        }
        let hash = if checksum {
            Some(hash_member(archive, &member, cancel, budget, progress).await?)
        } else {
            None
        };
        files.insert(relative.to_owned(), (member.size, member.modified, hash));
        progress(budget.progress());
    }
    let after = tokio::fs::symlink_metadata(archive).await?;
    if !after.is_file() || before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(error("Archive changed during comparison; compare again"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(error("Archive was replaced during comparison"));
        }
    }
    budget.check(cancel)?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_listing_are_strict_and_literal() {
        assert_eq!(
            parse_uri("archive:///a.zip#folder/").unwrap(),
            ("/a.zip", "folder/".into())
        );
        for uri in [
            "archive://relative#",
            "archive:///a.zip#../outside",
            "archive:///a.zip#/absolute",
        ] {
            assert!(parse_uri(uri).is_err());
        }
        for path in [
            "@listfile",
            "../escape",
            "/absolute",
            "C:/drive",
            "folder//file",
            "folder\\file",
            "folder/./file",
            "line\nfile",
        ] {
            assert!(!safe_member(path));
        }
        let member = "Path = folder/literal*?.txt\nSize = 3\nModified = 2026-01-01 00:00:00\nAttributes = A -rw-r--r--\n";
        assert_eq!(
            parse_listing(member).unwrap()[0].path,
            "folder/literal*?.txt"
        );
        assert!(parse_listing(&format!("{member}\n{member}")).is_err());
        assert!(parse_listing("Path = link\nSize = 1\nSymbolic Link = outside\n").is_err());
        assert!(parse_listing("Path = x\nSize = nope\n").is_err());
        assert!(parse_listing("Path = x\nSize = 1\nPath = other\n").is_err());
        assert!(parse_listing("Path = file\nSize = 1\n\nPath = file/child\nSize = 1\n").is_err());
    }

    #[tokio::test]
    async fn subprocess_output_is_bounded() {
        assert_eq!(read_capped(&b"abc"[..], 3).await.unwrap(), b"abc");
        assert!(read_capped(&b"abcd"[..], 3).await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires explicitly available 7z; disposable local archive only"]
    async fn real_archive_comparison_is_literal_read_only_and_exclusion_aware() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        std::fs::write(dir.path().join("folder/literal*?.txt"), b"abc").unwrap();
        std::fs::write(dir.path().join("folder/skip.txt"), b"skip").unwrap();
        for (filename, kind) in [
            ("fixture.zip", "-tzip"),
            ("fixture.7z", "-t7z"),
            ("fixture.tar", "-ttar"),
        ] {
            let archive = dir.path().join(filename);
            let status = Command::new("7z")
                .current_dir(dir.path())
                .args(["a", kind, filename, "folder"])
                .stdout(Stdio::null())
                .status()
                .await
                .unwrap();
            assert!(status.success());
            let original = std::fs::read(&archive).unwrap();
            let files = collect(
                &format!("archive://{}#folder", archive.display()),
                true,
                &AtomicBool::new(false),
                &["skip.txt".into()],
                &ScanBudget::new(),
                &|_| {},
            )
            .await
            .unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(
                files["literal*?.txt"].2.as_deref(),
                Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            );
            assert_eq!(std::fs::read(&archive).unwrap(), original);
            let uri = format!("archive://{}#", archive.display());
            let cancelled = collect(
                &uri,
                true,
                &AtomicBool::new(true),
                &[],
                &ScanBudget::new(),
                &|_| {},
            )
            .await;
            assert!(matches!(cancelled, Err(FmError::ScanCancelled)));
            let limits = crate::s3::scan::ScanLimits {
                max_objects: 1,
                ..crate::s3::scan::ScanLimits::DEFAULT
            };
            assert!(matches!(
                collect(
                    &uri,
                    false,
                    &AtomicBool::new(false),
                    &[],
                    &ScanBudget::with_limits(limits),
                    &|_| {}
                )
                .await,
                Err(FmError::ScanLimited(_))
            ));
            assert!(collect(
                &format!("{uri}missing"),
                false,
                &AtomicBool::new(false),
                &[],
                &ScanBudget::new(),
                &|_| {}
            )
            .await
            .is_err());
        }
    }
}
