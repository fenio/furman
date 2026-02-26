use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::models::FmError;
use super::helpers::s3err;

// ── State ────────────────────────────────────────────────────────────────────

pub struct S3State(pub Mutex<HashMap<String, S3Connection>>);

pub struct S3Connection {
    pub client: S3Client,
    pub bucket: String,
    pub region: String,
    pub sdk_config: aws_config::SdkConfig,
}

// ── Client Builder ──────────────────────────────────────────────────────────

/// Build an S3 client from credentials without storing it in state.
pub async fn build_s3_client(
    region: &str,
    endpoint: Option<&str>,
    profile: Option<&str>,
    access_key: Option<&str>,
    secret_key: Option<&str>,
    role_arn: Option<&str>,
    external_id: Option<&str>,
    session_name: Option<&str>,
    session_duration_secs: Option<i32>,
    use_transfer_acceleration: Option<bool>,
    anonymous: Option<bool>,
) -> Result<(S3Client, aws_config::SdkConfig), FmError> {
    let mut loader = if anonymous.unwrap_or(false) {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .no_credentials()
    } else if let (Some(ak), Some(sk)) = (access_key, secret_key) {
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

    let base_config = loader.load().await;

    // If a role ARN is provided, assume the role via STS
    let final_config = if let Some(arn) = role_arn {
        let sts_client = aws_sdk_sts::Client::new(&base_config);
        let mut assume_req = sts_client
            .assume_role()
            .role_arn(arn)
            .role_session_name(session_name.unwrap_or("furman-session"));

        if let Some(ext_id) = external_id {
            if !ext_id.is_empty() {
                assume_req = assume_req.external_id(ext_id);
            }
        }
        if let Some(duration) = session_duration_secs {
            assume_req = assume_req.duration_seconds(duration);
        }

        let assume_resp = assume_req
            .send()
            .await
            .map_err(|e| s3err(format!("AssumeRole failed: {}", e)))?;

        let sts_creds = assume_resp
            .credentials()
            .ok_or_else(|| s3err("AssumeRole returned no credentials"))?;

        let expiry = std::time::SystemTime::try_from(sts_creds.expiration().clone()).ok();

        let assumed_creds = aws_credential_types::Credentials::new(
            sts_creds.access_key_id(),
            sts_creds.secret_access_key(),
            Some(sts_creds.session_token().to_string()),
            expiry,
            "furman-assume-role",
        );

        // Rebuild config with assumed credentials
        let mut assumed_loader = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .credentials_provider(assumed_creds);

        if let Some(ep) = endpoint {
            if !ep.is_empty() {
                assumed_loader = assumed_loader.endpoint_url(ep);
            }
        }

        assumed_loader.load().await
    } else {
        base_config
    };

    let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&final_config);
    if endpoint.is_some_and(|ep| !ep.is_empty()) {
        s3_config_builder = s3_config_builder.force_path_style(true);
    }
    if use_transfer_acceleration.unwrap_or(false) && endpoint.is_none_or(|ep| ep.is_empty()) {
        s3_config_builder.set_accelerate(Some(true));
    }
    Ok((S3Client::from_conf(s3_config_builder.build()), final_config))
}
