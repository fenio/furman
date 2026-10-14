//! Bounded, read-only live content for comparison diffs. No temporary files.
use crate::{models::FmError, s3::S3State};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::io::AsyncReadExt;

const LIMIT: usize = 512 * 1024;

#[derive(Deserialize)]
pub struct DiffSource {
    backend: String,
    path: String,
    connection_id: String,
}

#[derive(Serialize)]
pub struct DiffContent {
    pub text: Option<String>,
    pub bytes: usize,
}

fn decode(bytes: Vec<u8>) -> Result<DiffContent, FmError> {
    if bytes.len() > LIMIT {
        return Err(FmError::Other("Text diff supports files up to 512 KiB per side. Download larger files to compare externally.".into()));
    }
    let size = bytes.len();
    let binary = bytes
        .iter()
        .any(|byte| matches!(byte, 0..=8 | 11..=12 | 14..=31 | 127));
    let text = if binary {
        None
    } else {
        String::from_utf8(bytes).ok()
    };
    Ok(DiffContent { text, bytes: size })
}

#[tauri::command]
pub async fn comparison_read_file(
    source: DiffSource,
    state: State<'_, S3State>,
) -> Result<DiffContent, FmError> {
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        read_source(source, state),
    )
    .await
    .map_err(|_| FmError::Other("Reading the diff file timed out.".into()))?
}

async fn read_source(
    source: DiffSource,
    state: State<'_, S3State>,
) -> Result<DiffContent, FmError> {
    match source.backend.as_str() {
        "local" => decode(read_local(&source.path).await?),
        "s3" => {
            let (client, bucket) = {
                let connections = state
                    .0
                    .lock()
                    .map_err(|_| FmError::Other("Connection state unavailable".into()))?;
                let connection = connections.get(&source.connection_id).ok_or_else(|| {
                    FmError::Other(
                        "S3 connection is no longer available. Reconnect and compare again.".into(),
                    )
                })?;
                (connection.client.clone(), connection.bucket.clone())
            };
            read_s3_file(&client, &bucket, &source.path).await
        }
        _ => {
            return Err(FmError::Other(
                "Text diff supports local and S3 files.".into(),
            ))
        }
    }
}

pub async fn read_s3_file(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    path: &str,
) -> Result<DiffContent, FmError> {
    let prefix = format!("s3://{bucket}/");
    let key = path
        .strip_prefix(&prefix)
        .ok_or_else(|| FmError::Other("Diff path does not belong to its S3 connection.".into()))?;
    let head = client.head_object().bucket(bucket).key(key).send().await
        .map_err(|_| FmError::S3("Could not inspect the S3 object for diff. Check permissions and the encryption key.".into()))?;
    if head.content_length() == Some(0) {
        return decode(Vec::new());
    }
    if head
        .content_length()
        .is_some_and(|size| size > LIMIT as i64)
    {
        return Err(FmError::Other("Text diff supports files up to 512 KiB per side. Download larger files to compare externally.".into()));
    }
    // The connection's SSE-C interceptor applies the same read headers.
    let response = client.get_object().bucket(bucket).key(key).range(format!("bytes=0-{LIMIT}"))
        .send().await.map_err(|_| FmError::S3("Could not read the S3 object for diff. Check permissions and the encryption key; reconnect if needed.".into()))?;
    let mut bytes = Vec::new();
    response
        .body
        .into_async_read()
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    decode(bytes)
}

async fn read_local(path: &str) -> Result<Vec<u8>, FmError> {
    let file = tokio::fs::File::open(path).await?;
    if !file.metadata().await?.is_file() {
        return Err(FmError::Other(
            "Only regular files support text diff.".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_binary_and_size_boundaries() {
        assert_eq!(
            decode(b"hello\n".to_vec()).unwrap().text.as_deref(),
            Some("hello\n")
        );
        assert_eq!(decode(Vec::new()).unwrap().text.as_deref(), Some(""));
        assert!(decode(vec![0]).unwrap().text.is_none());
        assert!(decode(vec![0xff]).unwrap().text.is_none());
        assert!(decode(vec![1, 2, 3]).unwrap().text.is_none());
        assert!(decode(vec![b'a'; LIMIT]).is_ok());
        assert!(decode(vec![b'a'; LIMIT + 1]).is_err());
    }

    #[tokio::test]
    async fn local_reads_are_bounded_read_only_and_report_missing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file");
        let data = vec![b'a'; LIMIT * 2];
        std::fs::write(&path, &data).unwrap();
        let bytes = read_local(path.to_str().unwrap()).await.unwrap();
        assert_eq!(bytes.len(), LIMIT + 1);
        assert!(decode(bytes).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert!(read_local(directory.path().to_str().unwrap())
            .await
            .is_err());
        assert!(
            read_local(directory.path().join("missing").to_str().unwrap())
                .await
                .is_err()
        );
    }
}
