//! Content verification deliberately does not interpret ETags as checksums.
use crate::models::FmError;
use aws_sdk_s3::Client;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};

pub fn check_cancelled(cancel: &AtomicBool) -> Result<(), FmError> {
    if cancel.load(Ordering::Relaxed) {
        Err(FmError::Other("Operation cancelled".into()))
    } else {
        Ok(())
    }
}

/// Hash a streamed GET with bounded memory. If supplied, the listing ETag pins
/// the read via If-Match; it is an opaque validator, never a content digest.
pub async fn object_sha256(
    client: &Client,
    bucket: &str,
    key: &str,
    expected_size: u64,
    etag: Option<&str>,
    cancel: &AtomicBool,
) -> Result<String, FmError> {
    object_sha256_bounded(client, bucket, key, expected_size, etag, cancel, None).await
}

pub async fn object_sha256_bounded(
    client: &Client,
    bucket: &str,
    key: &str,
    expected_size: u64,
    etag: Option<&str>,
    cancel: &AtomicBool,
    scan: Option<(
        &super::scan::ScanBudget,
        &(dyn Fn(super::scan::ScanProgress) + Send + Sync),
    )>,
) -> Result<String, FmError> {
    if let Some((budget, _)) = scan {
        budget.check(cancel)?;
    } else {
        check_cancelled(cancel)?;
    }
    let request = async {
        client
            .get_object()
            .bucket(bucket)
            .key(key)
            .set_if_match(etag.map(str::to_owned))
            .send()
            .await
            .map_err(|e| super::errors::s3_sdk_error("GetObject (SHA-256 verification)", &e))
    };
    let response = if let Some((budget, on_progress)) = scan {
        let response = budget.request(cancel, request).await?;
        on_progress(budget.progress());
        response
    } else {
        request.await?
    };
    let mut body = response.body;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    loop {
        let read = async {
            body.try_next()
                .await
                .map_err(|e| FmError::S3(format!("Cannot read {bucket}/{key}: {e}")))
        };
        let chunk = if let Some((budget, _)) = scan {
            budget.wait(cancel, read).await?
        } else {
            read.await?
        };
        let Some(chunk) = chunk else { break };
        if let Some((budget, _)) = scan {
            budget.check(cancel)?;
        } else {
            check_cancelled(cancel)?;
        }
        bytes += chunk.len() as u64;
        digest.update(&chunk);
        if let Some((budget, on_progress)) = scan {
            budget.bytes(chunk.len() as u64, true);
            if budget.progress().bytes_read % (1024 * 1024) < chunk.len() as u64 {
                on_progress(budget.progress());
            }
        }
    }
    if let Some((budget, on_progress)) = scan {
        budget.check(cancel)?;
        on_progress(budget.progress());
    } else {
        check_cancelled(cancel)?;
    }
    if bytes != expected_size {
        return Err(FmError::S3(format!(
            "Object changed or incomplete read while verifying {bucket}/{key}: expected {expected_size} bytes, read {bytes}"
        )));
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Copy preserves a selected folder's name and descendants. Sync instead
/// supplies its source root so all relative paths remain relative to that root.
pub fn destination_key(
    key: &str,
    selected_key: &str,
    source_prefix: Option<&str>,
    dest_prefix: &str,
) -> Result<String, FmError> {
    let root = if let Some(prefix) = source_prefix {
        prefix.trim_end_matches('/')
    } else {
        selected_key
            .trim_end_matches('/')
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent)
    };
    let relative = if root.is_empty() {
        key
    } else {
        key.strip_prefix(&format!("{root}/"))
            .ok_or_else(|| FmError::S3(format!("Source key {key} is outside prefix {root}")))?
    };
    if relative.is_empty() {
        return Err(FmError::S3("Empty destination object key".into()));
    }
    // Do not trim source keys: spaces and repeated slashes are valid S3 names.
    let dest = dest_prefix.trim_end_matches('/');
    Ok(if dest.is_empty() {
        relative.to_owned()
    } else {
        format!("{dest}/{relative}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_preserves_folder_and_duplicate_basenames() {
        assert_eq!(
            destination_key("src/folder/a/name.txt", "src/folder/", None, "dest/").unwrap(),
            "dest/folder/a/name.txt"
        );
        assert_eq!(
            destination_key("src/folder/b/name.txt", "src/folder/", None, "dest/").unwrap(),
            "dest/folder/b/name.txt"
        );
        assert_eq!(
            destination_key("src/file.txt", "src/file.txt", None, "").unwrap(),
            "file.txt"
        );
    }

    #[test]
    fn sync_preserves_paths_relative_to_root() {
        assert_eq!(
            destination_key("src/a/b.txt", "src/a/b.txt", Some("src/"), "dest").unwrap(),
            "dest/a/b.txt"
        );
        assert_eq!(
            destination_key("a/b.txt", "a/b.txt", Some(""), "").unwrap(),
            "a/b.txt"
        );
        assert!(
            destination_key("src-other/b.txt", "src-other/b.txt", Some("src"), "dest").is_err()
        );
        assert_eq!(
            destination_key("src/a// space.txt", "src/", Some("src"), "dest").unwrap(),
            "dest/a// space.txt"
        );
    }
}
