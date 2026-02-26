use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::models::FmError;

// ── State ────────────────────────────────────────────────────────────────────

pub struct S3State(pub Mutex<HashMap<String, S3Connection>>);

pub struct S3Connection {
    pub client: S3Client,
    pub bucket: String,
    pub region: String,
}

// ── Client Builder ──────────────────────────────────────────────────────────

/// Build an S3 client from credentials without storing it in state.
pub async fn build_s3_client(
    region: &str,
    endpoint: Option<&str>,
    profile: Option<&str>,
    access_key: Option<&str>,
    secret_key: Option<&str>,
) -> Result<S3Client, FmError> {
    let mut loader = if let (Some(ak), Some(sk)) = (access_key, secret_key) {
        let creds = aws_credential_types::Credentials::new(
            ak.to_string(),
            sk.to_string(),
            None,
            None,
            "furman-manual",
        );
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .credentials_provider(creds)
    } else if let Some(prof) = profile {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .profile_name(prof)
    } else {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
    };

    if let Some(ep) = endpoint {
        if !ep.is_empty() {
            loader = loader.endpoint_url(ep);
        }
    }

    let config = loader.load().await;

    let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&config);
    if endpoint.is_some_and(|ep| !ep.is_empty()) {
        s3_config_builder = s3_config_builder.force_path_style(true);
    }
    Ok(S3Client::from_conf(s3_config_builder.build()))
}
