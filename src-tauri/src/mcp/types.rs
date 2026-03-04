use crate::models::S3Tag;
use schemars::JsonSchema;
use serde::Deserialize;

// ── S3 Operations ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3ConnectParams {
    #[schemars(description = "S3 bucket name")]
    pub bucket: String,
    #[schemars(description = "AWS region (e.g. 'us-east-1')")]
    pub region: String,
    #[schemars(description = "Custom S3 endpoint URL (for MinIO, etc.)")]
    pub endpoint: Option<String>,
    #[schemars(description = "AWS credentials profile name")]
    pub profile: Option<String>,
    #[schemars(description = "AWS access key ID")]
    pub access_key: Option<String>,
    #[schemars(description = "AWS secret access key")]
    pub secret_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3ConnectionIdParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3ListObjectsParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 key prefix to list (e.g. 'folder/'). Default: '' (root)")]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3ObjectKeyParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key")]
    pub key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3DownloadParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key to download")]
    pub key: String,
    #[schemars(description = "Local file path to save to")]
    pub local_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3UploadParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "Local file path to upload")]
    pub local_path: String,
    #[schemars(description = "S3 destination key (e.g. 'folder/file.txt')")]
    pub key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3DeleteObjectsParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object keys to delete")]
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3CopyObjectParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "Source object key")]
    pub source: String,
    #[schemars(description = "Destination object key")]
    pub dest: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3PutObjectTagsParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key")]
    pub key: String,
    #[schemars(description = "Tags to set (key-value pairs)")]
    pub tags: Vec<S3Tag>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3PresignUrlParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key")]
    pub key: String,
    #[schemars(description = "URL expiration time in seconds. Default: 3600")]
    pub expires_secs: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3ChangeStorageClassParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key")]
    pub key: String,
    #[schemars(
        description = "Target storage class (STANDARD, STANDARD_IA, ONEZONE_IA, GLACIER, DEEP_ARCHIVE, etc.)"
    )]
    pub storage_class: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct S3PutTextParams {
    #[schemars(description = "Connection ID returned by s3_connect")]
    pub connection_id: String,
    #[schemars(description = "S3 object key")]
    pub key: String,
    #[schemars(description = "Text content to write")]
    pub content: String,
}

// ── SFTP Operations ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpConnectParams {
    #[schemars(description = "SSH hostname")]
    pub host: String,
    #[schemars(description = "SSH port. Default: 22")]
    pub port: Option<u16>,
    #[schemars(description = "SSH username")]
    pub username: String,
    #[schemars(description = "Authentication method: 'password' or 'key'")]
    pub auth_method: String,
    #[schemars(description = "Password (for 'password' auth)")]
    pub password: Option<String>,
    #[schemars(description = "Path to private key file (for 'key' auth)")]
    pub key_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpConnectionIdParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpListDirParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Remote directory path to list")]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpTransferParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Remote file paths")]
    pub remote_paths: Vec<String>,
    #[schemars(description = "Local destination directory")]
    pub local_dest: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpUploadParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Local file paths to upload")]
    pub local_paths: Vec<String>,
    #[schemars(description = "Remote destination directory")]
    pub remote_dest: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpDeleteParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Remote paths to delete")]
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpCreateFolderParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Remote directory path to create")]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SftpPutTextParams {
    #[schemars(description = "Connection ID returned by sftp_connect")]
    pub connection_id: String,
    #[schemars(description = "Remote file path")]
    pub path: String,
    #[schemars(description = "Text content to write")]
    pub content: String,
}
