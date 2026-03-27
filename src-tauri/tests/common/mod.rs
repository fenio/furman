use app_lib::models::ProgressEvent;
use app_lib::s3::client::build_s3_client;
use app_lib::s3::service::{self, S3Service};
use app_lib::sftp::client::build_sftp_client;
use app_lib::sftp::service::SftpService;
use aws_sdk_s3::Client as S3Client;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Configuration for connecting to MinIO or real AWS.
///
/// Supports two sets of env vars (first match wins):
///   S3_TEST_ENDPOINT / MINIO_ENDPOINT    — empty or unset = real AWS
///   S3_TEST_ACCESS_KEY / MINIO_ACCESS_KEY
///   S3_TEST_SECRET_KEY / MINIO_SECRET_KEY
///   S3_TEST_REGION / MINIO_REGION
pub struct MinioConfig {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
}

impl MinioConfig {
    /// Read config from environment with sensible defaults for local MinIO.
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("S3_TEST_ENDPOINT")
                .or_else(|_| std::env::var("MINIO_ENDPOINT"))
                .unwrap_or_default(),
            access_key: std::env::var("S3_TEST_ACCESS_KEY")
                .or_else(|_| std::env::var("MINIO_ACCESS_KEY"))
                .unwrap_or_else(|_| "minioadmin".to_string()),
            secret_key: std::env::var("S3_TEST_SECRET_KEY")
                .or_else(|_| std::env::var("MINIO_SECRET_KEY"))
                .unwrap_or_else(|_| "minioadmin".to_string()),
            region: std::env::var("S3_TEST_REGION")
                .or_else(|_| std::env::var("MINIO_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
        }
    }

    /// True when running against real AWS (no custom endpoint).
    #[allow(dead_code)]
    pub fn is_aws(&self) -> bool {
        self.endpoint.is_empty()
    }
}

/// Test context that owns a unique bucket and provides an S3Service.
/// Each test gets its own TestContext for isolation.
#[allow(dead_code)]
pub struct TestContext {
    pub service: S3Service,
    pub bucket: String,
    pub client: S3Client,
    pub config: MinioConfig,
    pub sdk_config: aws_config::SdkConfig,
    extra_buckets: Vec<String>,
}

impl TestContext {
    /// Create a new test context with a unique bucket.
    pub async fn new() -> Self {
        let config = MinioConfig::from_env();
        let endpoint = if config.endpoint.is_empty() {
            None
        } else {
            Some(config.endpoint.as_str())
        };
        let (client, sdk_config) = build_s3_client(
            &config.region,
            endpoint,
            None,
            Some(&config.access_key),
            Some(&config.secret_key),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to build S3 client for testing");

        let bucket = format!("test-{}", Uuid::new_v4());

        // Create the test bucket (uses service helper to handle LocationConstraint)
        service::create_bucket(&client, &bucket, &config.region)
            .await
            .expect("Failed to create test bucket");

        let service = S3Service::new(client.clone(), bucket.clone());

        Self {
            service,
            bucket,
            client,
            config,
            sdk_config,
            extra_buckets: Vec::new(),
        }
    }

    /// Create a new test context with a unique bucket that has Object Lock enabled.
    /// Object Lock requires versioning, which is automatically enabled.
    pub async fn new_with_object_lock() -> Self {
        let config = MinioConfig::from_env();
        let endpoint = if config.endpoint.is_empty() {
            None
        } else {
            Some(config.endpoint.as_str())
        };
        let (client, sdk_config) = build_s3_client(
            &config.region,
            endpoint,
            None,
            Some(&config.access_key),
            Some(&config.secret_key),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to build S3 client for testing");

        let bucket = format!("test-lock-{}", Uuid::new_v4());

        // Create bucket with Object Lock enabled
        let mut req = client
            .create_bucket()
            .bucket(&bucket)
            .object_lock_enabled_for_bucket(true);

        if config.region != "us-east-1" {
            let constraint = aws_sdk_s3::types::CreateBucketConfiguration::builder()
                .location_constraint(aws_sdk_s3::types::BucketLocationConstraint::from(
                    config.region.as_str(),
                ))
                .build();
            req = req.create_bucket_configuration(constraint);
        }

        req.send()
            .await
            .expect("Failed to create Object Lock test bucket");

        let service = S3Service::new(client.clone(), bucket.clone());

        Self {
            service,
            bucket,
            client,
            config,
            sdk_config,
            extra_buckets: Vec::new(),
        }
    }

    /// Create an additional bucket (for cross-bucket tests). Returns the bucket name.
    pub async fn create_extra_bucket(&mut self) -> String {
        let bucket = format!("test-extra-{}", Uuid::new_v4());
        service::create_bucket(&self.client, &bucket, &self.config.region)
            .await
            .expect("Failed to create extra test bucket");
        self.extra_buckets.push(bucket.clone());
        bucket
    }

    /// Convenience: put a small object in the test bucket.
    pub async fn put_object(&self, key: &str, data: &[u8]) {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.to_vec().into())
            .send()
            .await
            .expect("Failed to put test object");
    }

    /// Convenience: put a small object in a specific bucket.
    #[allow(dead_code)]
    pub async fn put_object_in_bucket(&self, bucket: &str, key: &str, data: &[u8]) {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(data.to_vec().into())
            .send()
            .await
            .expect("Failed to put test object in bucket");
    }

    /// Clean up: delete all objects and buckets created by this context.
    pub async fn cleanup(self) {
        // Delete all objects in the main bucket
        Self::delete_all_objects(&self.client, &self.bucket).await;

        // Delete main bucket
        let _ = self
            .client
            .delete_bucket()
            .bucket(&self.bucket)
            .send()
            .await;

        // Delete extra buckets
        for bucket in &self.extra_buckets {
            Self::delete_all_objects(&self.client, bucket).await;
            let _ = self.client.delete_bucket().bucket(bucket).send().await;
        }
    }

    /// Delete all objects (including versions) in a bucket.
    async fn delete_all_objects(client: &S3Client, bucket: &str) {
        // First try to delete all object versions (for versioned buckets)
        let mut key_marker: Option<String> = None;
        let mut vid_marker: Option<String> = None;

        loop {
            let mut req = client.list_object_versions().bucket(bucket);
            if let Some(km) = &key_marker {
                req = req.key_marker(km);
            }
            if let Some(vm) = &vid_marker {
                req = req.version_id_marker(vm);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(_) => break,
            };

            let mut to_delete = Vec::new();

            for v in resp.versions() {
                if let (Some(k), Some(vid)) = (v.key(), v.version_id()) {
                    to_delete.push(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key(k)
                            .version_id(vid)
                            .build()
                            .unwrap(),
                    );
                }
            }

            for dm in resp.delete_markers() {
                if let (Some(k), Some(vid)) = (dm.key(), dm.version_id()) {
                    to_delete.push(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key(k)
                            .version_id(vid)
                            .build()
                            .unwrap(),
                    );
                }
            }

            if !to_delete.is_empty() {
                for chunk in to_delete.chunks(1000) {
                    let delete = aws_sdk_s3::types::Delete::builder()
                        .set_objects(Some(chunk.to_vec()))
                        .build()
                        .unwrap();
                    let _ = client
                        .delete_objects()
                        .bucket(bucket)
                        .delete(delete)
                        .send()
                        .await;
                }
            }

            if resp.is_truncated() == Some(true) {
                key_marker = resp.next_key_marker().map(|s| s.to_string());
                vid_marker = resp.next_version_id_marker().map(|s| s.to_string());
            } else {
                break;
            }
        }

        // Also try plain list + delete for non-versioned buckets
        loop {
            let resp = match client.list_objects_v2().bucket(bucket).send().await {
                Ok(r) => r,
                Err(_) => break,
            };

            let objects: Vec<_> = resp
                .contents()
                .iter()
                .filter_map(|o| {
                    Some(
                        aws_sdk_s3::types::ObjectIdentifier::builder()
                            .key(o.key()?)
                            .build()
                            .unwrap(),
                    )
                })
                .collect();

            if objects.is_empty() {
                break;
            }

            let delete = aws_sdk_s3::types::Delete::builder()
                .set_objects(Some(objects))
                .build()
                .unwrap();
            let _ = client
                .delete_objects()
                .bucket(bucket)
                .delete(delete)
                .send()
                .await;
        }
    }
}

// ── SFTP Test Context ────────────────────────────────────────────────────────

/// Configuration for connecting to an SFTP test server.
///
/// Reads from env vars:
///   SFTP_TEST_HOST (default: localhost)
///   SFTP_TEST_PORT (default: 2222)
///   SFTP_TEST_USER (default: testuser)
///   SFTP_TEST_PASS (default: testpass)
#[allow(dead_code)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

impl SftpConfig {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("SFTP_TEST_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("SFTP_TEST_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(2222),
            user: std::env::var("SFTP_TEST_USER").unwrap_or_else(|_| "testuser".to_string()),
            pass: std::env::var("SFTP_TEST_PASS").unwrap_or_else(|_| "testpass".to_string()),
        }
    }
}

/// Test context that owns a unique directory on the SFTP server.
/// Each test gets its own SftpTestContext for isolation.
#[allow(dead_code)]
pub struct SftpTestContext {
    pub service: SftpService,
    pub test_dir: String,
    pub host: String,
    pub port: u16,
}

#[allow(dead_code)]
impl SftpTestContext {
    /// Create a new SFTP test context with a unique directory.
    pub async fn new() -> Self {
        let config = SftpConfig::from_env();
        let conn = build_sftp_client(
            &config.host,
            config.port,
            &config.user,
            "password",
            Some(&config.pass),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to connect to SFTP test server");

        let service = SftpService::new(conn.session.clone(), config.host.clone(), config.port, conn.operation_timeout_secs);

        // Use the resolved home dir (accounts for chroot jails like atmoz/sftp)
        let home = conn.home_dir.trim_end_matches('/');
        let test_dir = format!("{}/upload/{}", home, Uuid::new_v4());
        service
            .create_folder(&test_dir)
            .await
            .expect("Failed to create SFTP test directory");

        Self {
            service,
            test_dir,
            host: config.host,
            port: config.port,
        }
    }

    /// Write a file inside the test directory.
    pub async fn put_file(&self, name: &str, data: &[u8]) {
        let path = format!("{}/{}", self.test_dir, name);
        let mut file = self
            .service
            .session
            .create(&path)
            .await
            .expect("Failed to create test file");
        file.write_all(data)
            .await
            .expect("Failed to write test file data");
    }

    /// Write a file at an arbitrary subpath, creating parent dirs.
    pub async fn put_file_at(&self, relative_path: &str, data: &[u8]) {
        let path = format!("{}/{}", self.test_dir, relative_path);
        // Ensure parent dirs exist
        if let Some((parent, _)) = path.rsplit_once('/') {
            self.ensure_dir(parent).await;
        }
        let mut file = self
            .service
            .session
            .create(&path)
            .await
            .expect("Failed to create test file");
        file.write_all(data)
            .await
            .expect("Failed to write test file data");
    }

    /// Create a subdirectory inside the test directory.
    pub async fn mkdir(&self, name: &str) {
        let path = format!("{}/{}", self.test_dir, name);
        self.service
            .create_folder(&path)
            .await
            .expect("Failed to create test subdirectory");
    }

    /// Ensure a remote directory and all parents exist.
    async fn ensure_dir(&self, path: &str) {
        if self.service.session.try_exists(path).await.unwrap_or(false) {
            return;
        }
        if let Some((parent, _)) = path.rsplit_once('/') {
            if !parent.is_empty() {
                Box::pin(self.ensure_dir(parent)).await;
            }
        }
        let _ = self.service.create_folder(path).await;
    }

    /// Clean up: delete the test directory and all its contents.
    pub async fn cleanup(self) {
        let _ = self.service.delete(&[self.test_dir.clone()]).await;
    }
}

// ── Local Filesystem Test Context ────────────────────────────────────────────

/// Test context for local filesystem integration tests.
/// Each test gets a fresh temp directory that is auto-cleaned on drop.
#[allow(dead_code)]
pub struct LocalTestContext {
    pub root: TempDir,
}

#[allow(dead_code)]
impl LocalTestContext {
    /// Create a new test context with a fresh temp directory.
    pub fn new() -> Self {
        Self {
            root: TempDir::new().expect("Failed to create temp dir"),
        }
    }

    /// Return the root path as a String.
    pub fn path_str(&self) -> String {
        self.root.path().to_string_lossy().into_owned()
    }

    /// Create a file at `rel` (relative to root) with the given data.
    /// Auto-creates parent directories.
    pub fn put_file(&self, rel: &str, data: &[u8]) {
        let path = self.root.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        std::fs::write(&path, data).expect("Failed to write test file");
    }

    /// Create a directory at `rel` (relative to root), including parents.
    pub fn mkdir(&self, rel: &str) {
        let path = self.root.path().join(rel);
        std::fs::create_dir_all(&path).expect("Failed to create directory");
    }

    /// Create a symlink at `link_rel` pointing to `target`.
    #[cfg(unix)]
    pub fn symlink(&self, target: &str, link_rel: &str) {
        let link_path = self.root.path().join(link_rel);
        if let Some(parent) = link_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        std::os::unix::fs::symlink(target, &link_path).expect("Failed to create symlink");
    }

    /// Create a standard test tree:
    /// ```text
    /// file1.txt         (11 bytes: "hello world")
    /// file2.txt         (7 bytes: "foo bar")
    /// .hidden           (6 bytes: "secret")
    /// empty.txt         (0 bytes)
    /// subdir/
    ///   nested.txt      (6 bytes: "nested")
    ///   deep/
    ///     deep.txt      (4 bytes: "deep")
    /// ```
    pub fn create_standard_tree(&self) {
        self.put_file("file1.txt", b"hello world");
        self.put_file("file2.txt", b"foo bar");
        self.put_file(".hidden", b"secret");
        self.put_file("empty.txt", b"");
        self.put_file("subdir/nested.txt", b"nested");
        self.put_file("subdir/deep/deep.txt", b"deep");
    }

    /// Create an empty `dst/` directory and return its path as a String.
    pub fn create_dest(&self) -> String {
        self.mkdir("dst");
        self.root.path().join("dst").to_string_lossy().into_owned()
    }

    /// Assert that a file at `rel` exists and has the given content.
    pub fn assert_file_content(&self, rel: &str, expected: &[u8]) {
        let path = self.root.path().join(rel);
        assert!(path.exists(), "Expected file to exist: {rel}");
        let actual = std::fs::read(&path).expect("Failed to read file");
        assert_eq!(actual, expected, "Content mismatch for {rel}");
    }

    /// Assert that a path at `rel` exists.
    pub fn assert_exists(&self, rel: &str) {
        let path = self.root.path().join(rel);
        assert!(path.exists(), "Expected path to exist: {rel}");
    }

    /// Assert that a path at `rel` does NOT exist.
    pub fn assert_not_exists(&self, rel: &str) {
        let path = self.root.path().join(rel);
        assert!(!path.exists(), "Expected path to NOT exist: {rel}");
    }

    /// Return absolute path for a relative path within the test root.
    pub fn abs(&self, rel: &str) -> String {
        self.root.path().join(rel).to_string_lossy().into_owned()
    }
}

// ── Progress helpers ─────────────────────────────────────────────────────────

/// Return a progress callback that discards all events.
#[allow(dead_code)]
pub fn noop_progress() -> impl Fn(ProgressEvent) {
    |_| {}
}

/// Return a progress callback that captures events, plus a handle to read them.
#[allow(dead_code)]
pub fn collect_progress() -> (Arc<Mutex<Vec<ProgressEvent>>>, impl Fn(ProgressEvent)) {
    let events: Arc<Mutex<Vec<ProgressEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let callback = move |event: ProgressEvent| {
        events_clone.lock().unwrap().push(event);
    };
    (events, callback)
}
