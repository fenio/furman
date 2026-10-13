//! SSE-C secrets are connection-local and never serialized into profiles.
use crate::models::FmError;
use aws_sdk_s3::{config::Intercept, Client};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::{
    interceptors::context::{
        BeforeDeserializationInterceptorContextMut, BeforeTransmitInterceptorContextMut,
        InterceptorContext,
    },
    orchestrator::Metadata,
    retries::classifiers::{ClassifyRetry, RetryAction},
    runtime_components::RuntimeComponents,
};
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::fmt;
use zeroize::Zeroizing;

const NAME: &str = "FurmanSseC";

#[derive(Clone, Debug)]
struct ReadWithoutKey;
impl Storable for ReadWithoutKey {
    type Storer = StoreReplace<Self>;
}

#[derive(Debug)]
struct RetryPlainRead;
impl ClassifyRetry for RetryPlainRead {
    fn name(&self) -> &'static str {
        "SseCPlainReadFallback"
    }
    fn classify_retry(&self, context: &InterceptorContext) -> RetryAction {
        if context
            .response()
            .is_some_and(|response| response.extension::<ReadWithoutKey>().is_some())
        {
            RetryAction::client_error()
        } else {
            RetryAction::NoActionIndicated
        }
    }
}

#[derive(Clone)]
struct SseCHeaders {
    key: Zeroizing<String>,
    md5: Zeroizing<String>,
}

impl fmt::Debug for SseCHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SseCHeaders([REDACTED])")
    }
}

impl SseCHeaders {
    fn new(key: &str) -> Result<Self, FmError> {
        let bytes = Zeroizing::new(STANDARD.decode(key).map_err(|_| {
            FmError::S3("SSE-C key must be base64-encoded, containing exactly 32 bytes".into())
        })?);
        if bytes.len() != 32 {
            return Err(FmError::S3(
                "SSE-C requires a 32-byte AES-256 key (base64-encoded)".into(),
            ));
        }
        Ok(Self {
            key: Zeroizing::new(STANDARD.encode(&*bytes)),
            md5: Zeroizing::new(STANDARD.encode(md5::compute(&*bytes).as_ref())),
        })
    }
}

impl Intercept for SseCHeaders {
    fn name(&self) -> &'static str {
        NAME
    }

    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let operation = config
            .load::<Metadata>()
            .map(|metadata| metadata.name())
            .unwrap_or("");
        let copy = matches!(operation, "CopyObject" | "UploadPartCopy");
        let plain_read = matches!(operation, "HeadObject" | "GetObject")
            && config.load::<ReadWithoutKey>().is_some();
        if !copy
            && !matches!(
                operation,
                "HeadObject"
                    | "GetObject"
                    | "PutObject"
                    | "CreateMultipartUpload"
                    | "UploadPart"
                    | "CompleteMultipartUpload"
            )
        {
            return Ok(());
        }
        let request = context.request_mut();
        if !request.uri().starts_with("https://") {
            return Err(
                "SSE-C requires HTTPS; refusing to send an encryption key over HTTP".into(),
            );
        }
        let headers = request.headers_mut();
        if plain_read {
            // Reads only: plain objects may reject SSE-C headers. Retrying
            // without them cannot decrypt an object whose key is missing/wrong.
            headers.remove("x-amz-server-side-encryption-customer-algorithm");
            headers.remove("x-amz-server-side-encryption-customer-key");
            headers.remove("x-amz-server-side-encryption-customer-key-md5");
            return Ok(());
        }
        headers.insert("x-amz-server-side-encryption-customer-algorithm", "AES256");
        headers.insert(
            "x-amz-server-side-encryption-customer-key",
            self.key.as_str().to_owned(),
        );
        headers.insert(
            "x-amz-server-side-encryption-customer-key-md5",
            self.md5.as_str().to_owned(),
        );
        if copy {
            // Only same-key/self-copy operations use CopyObject with SSE-C.
            // Cross-connection copies are forced through GET/PUT relay below.
            headers.insert(
                "x-amz-copy-source-server-side-encryption-customer-algorithm",
                "AES256",
            );
            headers.insert(
                "x-amz-copy-source-server-side-encryption-customer-key",
                self.key.as_str().to_owned(),
            );
            headers.insert(
                "x-amz-copy-source-server-side-encryption-customer-key-md5",
                self.md5.as_str().to_owned(),
            );
        }
        Ok(())
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let operation = config
            .load::<Metadata>()
            .map(|metadata| metadata.name())
            .unwrap_or("");
        if matches!(operation, "HeadObject" | "GetObject")
            && context.response().status().as_u16() == 400
            && config.load::<ReadWithoutKey>().is_none()
        {
            config.interceptor_state().store_put(ReadWithoutKey);
            context.response_mut().add_extension(ReadWithoutKey);
        }
        Ok(())
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime: &RuntimeComponents,
        _config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let request = context.request_mut();
        let headers = request.headers();
        if let Some(key) = headers.get("x-amz-server-side-encryption-customer-key") {
            let bytes = Zeroizing::new(
                STANDARD
                    .decode(key)
                    .map_err(|_| "Outgoing SSE-C key is not valid base64")?,
            );
            let supplied = headers
                .get("x-amz-server-side-encryption-customer-key-md5")
                .ok_or("Outgoing SSE-C request is missing its key-MD5 header")?;
            let supplied = STANDARD
                .decode(supplied)
                .map_err(|_| "Outgoing SSE-C key-MD5 header is not valid base64")?;
            if bytes.len() != 32 || supplied.as_slice() != md5::compute(&*bytes).as_ref() {
                return Err(
                    "Outgoing SSE-C key and key-MD5 headers are inconsistent; request blocked"
                        .into(),
                );
            }
            if headers.get("range") == Some("bytes=0-0") {
                // Values and fingerprints are deliberately not logged.
                log::info!("SSE-C diagnostic outgoing headers verified: decoded key length=32 bytes; key-MD5 matches=true; algorithm={}", headers.get("x-amz-server-side-encryption-customer-algorithm").unwrap_or("missing"));
            }
        }
        Ok(())
    }
}

pub fn configure(client: Client, key: Option<&str>) -> Result<Client, FmError> {
    match key {
        None => Ok(client),
        Some(key) => {
            let headers = SseCHeaders::new(key)?;
            Ok(Client::from_conf(
                client
                    .config()
                    .to_builder()
                    .interceptor(headers)
                    .retry_classifier(RetryPlainRead)
                    .build(),
            ))
        }
    }
}

pub fn enabled(client: &Client) -> bool {
    client
        .config()
        .interceptors()
        .any(|interceptor| interceptor.name() == NAME)
}

pub fn failure_hint(configured: bool) -> &'static str {
    if configured {
        "SSE-C is configured: verify the original key for this object/version. Some S3-compatible servers report a key-MD5 mismatch when the supplied key differs from the object's stored key, even if the request headers are internally consistent."
    } else {
        "If this object uses SSE-C, edit the connection and configure its original customer encryption key. HTTP 400 alone does not confirm encryption."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_md5_matches_independent_known_vector() {
        let key = STANDARD.encode(b"0123456789abcdef0123456789abcdef");
        let headers = SseCHeaders::new(&key).unwrap();
        // Independently calculated with Python hashlib, not this Rust MD5 crate.
        assert_eq!(headers.md5.as_str(), "hRasmdxgYDKV3nvbahU1MA==");
    }

    #[test]
    fn validates_key_and_redacts_debug_output() {
        let key = STANDARD.encode([42u8; 32]);
        let headers = SseCHeaders::new(&key).unwrap();
        assert_eq!(headers.key.as_str(), key);
        assert_eq!(
            headers.md5.as_str(),
            STANDARD.encode(md5::compute([42u8; 32]).as_ref())
        );
        assert!(!format!("{headers:?}").contains(&key));
        assert!(SseCHeaders::new("not-base64!").is_err());
        assert!(SseCHeaders::new(&STANDARD.encode([0u8; 31])).is_err());
    }

    fn test_client(
        endpoint: &str,
    ) -> (
        Client,
        aws_smithy_http_client::test_util::CaptureRequestReceiver,
    ) {
        let (http, request) = aws_smithy_http_client::test_util::capture_request(None);
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new(
                "test", "test", None, None, "test",
            ))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
            .http_client(http)
            .build();
        (Client::from_conf(config), request)
    }

    #[tokio::test]
    async fn encryption_headers_are_added_before_signing_for_object_operations() {
        let key = STANDARD.encode([42u8; 32]);
        for operation in [
            "head",
            "get",
            "put",
            "copy",
            "multipart",
            "part",
            "copy-part",
            "complete",
        ] {
            let (client, request) = test_client("https://sse-c.test");
            let client = configure(client, Some(&key)).unwrap();
            assert!(enabled(&client));
            match operation {
                "head" => {
                    let _ = client
                        .head_object()
                        .bucket("bucket")
                        .key("file")
                        .send()
                        .await;
                }
                "get" => {
                    let _ = client
                        .get_object()
                        .bucket("bucket")
                        .key("file")
                        .send()
                        .await;
                }
                "put" => {
                    let _ = client
                        .put_object()
                        .bucket("bucket")
                        .key("file")
                        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"data"))
                        .send()
                        .await;
                }
                "copy" => {
                    let _ = client
                        .copy_object()
                        .bucket("bucket")
                        .key("dest")
                        .copy_source("bucket/source")
                        .send()
                        .await;
                }
                "multipart" => {
                    let _ = client
                        .create_multipart_upload()
                        .bucket("bucket")
                        .key("file")
                        .send()
                        .await;
                }
                "part" => {
                    let _ = client
                        .upload_part()
                        .bucket("bucket")
                        .key("file")
                        .upload_id("id")
                        .part_number(1)
                        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"data"))
                        .send()
                        .await;
                }
                "copy-part" => {
                    let _ = client
                        .upload_part_copy()
                        .bucket("bucket")
                        .key("dest")
                        .copy_source("bucket/source")
                        .upload_id("id")
                        .part_number(1)
                        .send()
                        .await;
                }
                "complete" => {
                    let _ = client
                        .complete_multipart_upload()
                        .bucket("bucket")
                        .key("file")
                        .upload_id("id")
                        .send()
                        .await;
                }
                _ => unreachable!(),
            }
            let request = request.expect_request();
            let headers = request.headers();
            assert_eq!(
                headers.get("x-amz-server-side-encryption-customer-algorithm"),
                Some("AES256"),
                "{operation}"
            );
            assert_eq!(
                headers.get("x-amz-server-side-encryption-customer-key"),
                Some(key.as_str()),
                "{operation}"
            );
            assert_eq!(
                headers.get("x-amz-server-side-encryption-customer-key-md5"),
                Some(STANDARD.encode(md5::compute([42u8; 32]).as_ref()).as_str()),
                "{operation}"
            );
            assert!(
                headers
                    .get("authorization")
                    .unwrap()
                    .contains("x-amz-server-side-encryption-customer-key"),
                "{operation}: key must be signed"
            );
            if matches!(operation, "copy" | "copy-part") {
                assert_eq!(
                    headers.get("x-amz-copy-source-server-side-encryption-customer-key"),
                    Some(key.as_str())
                );
            }
        }
    }

    #[tokio::test]
    async fn bucket_operations_do_not_receive_encryption_secrets() {
        let (client, request) = test_client("https://sse-c.test");
        let client = configure(client, Some(&STANDARD.encode([42u8; 32]))).unwrap();
        let _ = client.list_objects_v2().bucket("bucket").send().await;
        assert_eq!(
            request
                .expect_request()
                .headers()
                .get("x-amz-server-side-encryption-customer-key"),
            None
        );
    }

    #[tokio::test]
    async fn keys_are_never_sent_over_http() {
        let (client, request) = test_client("http://sse-c.test");
        let client = configure(client, Some(&STANDARD.encode([42u8; 32]))).unwrap();
        assert!(client
            .head_object()
            .bucket("bucket")
            .key("file")
            .send()
            .await
            .is_err());
        request.expect_no_request();
    }

    #[tokio::test]
    async fn unconfigured_connections_remain_unencrypted() {
        let (client, request) = test_client("https://sse-c.test");
        let client = configure(client, None).unwrap();
        assert!(!enabled(&client));
        let _ = client
            .head_object()
            .bucket("bucket")
            .key("file")
            .send()
            .await;
        assert_eq!(
            request
                .expect_request()
                .headers()
                .get("x-amz-server-side-encryption-customer-key"),
            None
        );
    }
}
