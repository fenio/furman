use app_lib::s3::client::build_s3_client;
use app_lib::s3::service::{self, S3Service};
use aws_sdk_s3::Client as S3Client;
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
        let (client, _) = build_s3_client(
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
        let (client, _) = build_s3_client(
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
        )
        .await
        .expect("Failed to build S3 client for testing");

        let bucket = format!("test-lock-{}", Uuid::new_v4());

        // Create bucket with Object Lock enabled
        let mut req = client.create_bucket().bucket(&bucket).object_lock_enabled_for_bucket(true);

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
        let _ = self.client.delete_bucket().bucket(&self.bucket).send().await;

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
            let resp = match client
                .list_objects_v2()
                .bucket(bucket)
                .send()
                .await
            {
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
