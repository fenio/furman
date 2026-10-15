use super::*;

pub(crate) async fn fixture(
    config: &str,
    credentials: &str,
) -> (tempfile::TempDir, ProfilePaths, ProfileSet) {
    let root = tempfile::tempdir().unwrap();
    let paths = ProfilePaths {
        config: root.path().join("config"),
        credentials: root.path().join("credentials"),
    };
    std::fs::write(&paths.config, config).unwrap();
    std::fs::write(&paths.credentials, credentials).unwrap();
    let parsed = load(&paths).await.unwrap();
    (root, paths, parsed)
}

#[tokio::test]
async fn discovers_modern_legacy_and_role_chain_profiles_without_exporting_secrets() {
    let (_root, _paths, profiles) = fixture("[profile modern]\nsso_session = company\nsso_account_id = 123456789012\nsso_role_name = PrivateRole\nregion = eu-west-1\n[sso-session company]\nsso_start_url = https://PRIVATE.awsapps.com/start\nsso_region = us-east-2\n[profile legacy]\nsso_start_url = https://PRIVATE.awsapps.com/start\nsso_region = us-east-2\nsso_account_id = 123456789012\nsso_role_name = PrivateRole\n[profile chained]\nrole_arn = arn:aws:iam::123456789012:role/PrivateRole\nsource_profile = modern\n", "[default]\naws_access_key_id = SECRET_ACCESS\naws_secret_access_key = SECRET_KEY\naws_session_token = SECRET_TOKEN\n[linode]\naws_access_key_id = LINODE_ACCESS\naws_secret_access_key = SECRET_KEY\n").await;
    let report = summarize(&profiles).unwrap();
    assert_eq!(
        report
            .profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        ["chained", "default", "legacy", "linode", "modern"]
    );
    assert_eq!(
        describe(&profiles, "modern").region.as_deref(),
        Some("eu-west-1")
    ); // NOT sso_region.
    assert_eq!(
        describe(&profiles, "modern").login_profile.as_deref(),
        Some("modern")
    );
    assert_eq!(
        describe(&profiles, "chained").login_profile.as_deref(),
        Some("modern")
    );
    assert_eq!(
        describe(&profiles, "legacy").login_profile.as_deref(),
        Some("legacy")
    );
    assert!(!describe(&profiles, "linode").sso);
    let json = serde_json::to_string(&report).unwrap();
    for secret in [
        "SECRET",
        "LINODE_ACCESS",
        "123456789012",
        "PrivateRole",
        "awsapps.com",
    ] {
        assert!(
            !json.contains(secret),
            "Unexpected field in profile report: {secret}"
        );
    }
}

#[tokio::test]
async fn incomplete_mixed_endpoint_and_cyclic_profiles_do_not_offer_login() {
    let (_root, _paths, profiles) = fixture("[profile missing]\nsso_session = absent\nsso_account_id = account\nsso_role_name = role\n[profile complete]\nsso_start_url = https://example.awsapps.com/start\nsso_region = us-east-1\nsso_account_id = account\nsso_role_name = role\n[profile custom]\nrole_arn = arn:role\nsource_profile = complete\nendpoint_url = https://us-ord-1.linodeobjects.com\n[profile mixed]\nrole_arn = arn:role\nsource_profile = complete\ncredential_process = DO_NOT_EXECUTE\n[profile cycle-a]\nrole_arn = arn:role\nsource_profile = cycle-b\n[profile cycle-b]\nrole_arn = arn:role\nsource_profile = cycle-a\n", "[complete]\naws_access_key_id = SECRET\naws_secret_access_key = SECRET\n").await;
    for name in ["missing", "complete", "custom", "mixed", "cycle-a"] {
        let entry = describe(&profiles, name);
        assert!(entry.login_profile.is_none(), "{name}");
        assert!(entry.login_blocked_reason.is_some(), "{name}");
    }
    assert!(describe(&profiles, "custom").has_custom_endpoint);
}

#[tokio::test]
async fn discovery_never_executes_credential_process_and_uses_sdk_file_precedence() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("should-not-exist");
    let config = format!("[profile process]\ncredential_process = /usr/bin/touch {}\n[profile merge]\nregion = eu-west-1\n", marker.display());
    let (_files, _paths, profiles) = fixture(&config, "[merge]\nregion = ap-south-1\naws_access_key_id = SECRET\naws_secret_access_key = SECRET\n").await;
    assert_eq!(
        describe(&profiles, "merge").region.as_deref(),
        Some("ap-south-1")
    );
    assert!(!describe(&profiles, "process").sso);
    assert!(!marker.exists());
}

#[tokio::test]
async fn invalid_and_oversized_shared_files_fail_without_file_contents() {
    let root = tempfile::tempdir().unwrap();
    let paths = ProfilePaths {
        config: root.path().join("config"),
        credentials: root.path().join("missing"),
    };
    assert!(summarize(&load(&paths).await.unwrap())
        .unwrap()
        .profiles
        .is_empty());
    std::fs::write(
        &paths.config,
        "[profile bad\naws_secret_access_key=SECRET\n",
    )
    .unwrap();
    let error = load(&paths).await.err().unwrap().to_string();
    assert!(error.contains("parse"));
    assert!(!error.contains("SECRET"));
    std::fs::File::create(&paths.config)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert!(load(&paths)
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("2 MiB"));
    std::fs::remove_file(&paths.config).unwrap();
    std::fs::create_dir(&paths.config).unwrap();
    assert!(load(&paths).await.is_err());
}

#[tokio::test]
async fn special_files_and_excessive_profile_counts_fail_closed() {
    use nix::{sys::stat::Mode, unistd::mkfifo};
    let root = tempfile::tempdir().unwrap();
    let fifo = root.path().join("fifo");
    mkfifo(&fifo, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
    let paths = ProfilePaths {
        config: fifo,
        credentials: root.path().join("missing"),
    };
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), load(&paths))
            .await
            .unwrap()
            .is_err()
    );
    let config = (0..MAX_PROFILES + 1)
        .map(|index| format!("[profile p{index}]\nregion=us-east-1\n"))
        .collect::<String>();
    let (_root, _paths, profiles) = fixture(&config, "").await;
    assert!(summarize(&profiles).is_err());
}

#[test]
fn sso_hints_require_typed_credential_errors_and_never_dump_sources() {
    use aws_credential_types::provider::error::CredentialsError;
    let expired = CredentialsError::provider_error(std::io::Error::other(
        "the SSO token has expired and cannot be refreshed SECRET",
    ));
    assert_eq!(sso_credential_hint(&expired), Some(SSO_EXPIRED));
    let unavailable = CredentialsError::provider_error(std::io::Error::other(
        "failed to load the cached SSO token SECRET",
    ));
    assert_eq!(sso_credential_hint(&unavailable), Some(SSO_UNAVAILABLE));
    let config = CredentialsError::invalid_configuration(std::io::Error::other(
        "profile private has invalid SSO config SECRET",
    ));
    assert_eq!(sso_credential_hint(&config), Some(SSO_CONFIG));
    let linode = CredentialsError::provider_error(std::io::Error::other(
        "ExpiredToken: temporary session token expired",
    ));
    assert_eq!(sso_credential_hint(&linode), None);
    assert_eq!(
        sso_credential_hint(&std::io::Error::other("the SSO token has expired SECRET")),
        None
    );
}

#[test]
fn linode_and_akamai_service_errors_are_not_reclassified_as_sso_expiry() {
    use aws_sdk_s3::{error::SdkError, operation::head_bucket::HeadBucketError};
    use aws_smithy_runtime_api::http::{Response, StatusCode};
    use aws_smithy_types::{body::SdkBody, error::ErrorMetadata};
    for code in [
        "AccessDenied",
        "InvalidAccessKeyId",
        "ExpiredToken",
        "SignatureDoesNotMatch",
    ] {
        let metadata = ErrorMetadata::builder()
            .code(code)
            .message("Provider error mentions the SSO token has expired")
            .build();
        let error = SdkError::service_error(
            HeadBucketError::generic(metadata),
            Response::new(StatusCode::try_from(403).unwrap(), SdkBody::empty()),
        );
        let message =
            super::super::errors::s3_sdk_error("Linode/Akamai HeadBucket", &error).to_string();
        assert!(message.contains(code));
        assert!(message.contains("HTTP 403"));
        assert!(!message.contains("[AWS_SSO_"));
    }
}

#[test]
fn expired_sso_credentials_surface_through_sdk_without_raw_secrets() {
    use aws_credential_types::provider::error::CredentialsError;
    use aws_sdk_s3::{error::SdkError, operation::head_bucket::HeadBucketError};
    let credentials = CredentialsError::provider_error(std::io::Error::other(
        "the SSO token has expired SECRET_ACCESS_TOKEN",
    ));
    let error = SdkError::<HeadBucketError>::construction_failure(credentials);
    let message = super::super::errors::s3_sdk_error("HeadBucket", &error).to_string();
    assert!(message.contains("[AWS_SSO_SESSION_EXPIRED]"), "{message}");
    assert!(!message.contains("SECRET_ACCESS_TOKEN"));
}

#[test]
fn shared_file_paths_expand_home_and_keep_relative_overrides_consistent_with_cli() {
    let home = Path::new("/home/example");
    let cwd = Path::new("/work");
    assert_eq!(
        absolute_path(Path::new("~/config/custom"), Some(home), cwd).unwrap(),
        home.join("config/custom")
    );
    assert_eq!(
        absolute_path(Path::new("relative/config"), Some(home), cwd).unwrap(),
        cwd.join("relative/config")
    );
    assert_eq!(
        absolute_path(Path::new("/etc/aws/config"), None, cwd).unwrap(),
        Path::new("/etc/aws/config")
    );
    assert!(absolute_path(Path::new("~/config"), None, cwd).is_err());
}

#[test]
fn sso_role_credentials_unauthorized_is_distinct_from_s3_permission_denied() {
    use aws_credential_types::provider::error::CredentialsError;
    use aws_smithy_runtime_api::http::{Response, StatusCode};
    use aws_smithy_types::body::SdkBody;
    let unauthorized = aws_sdk_sso::types::error::UnauthorizedException::builder()
        .message("SECRET_TOKEN_INVALID")
        .build();
    let service = aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsError::UnauthorizedException(unauthorized);
    let error = aws_sdk_sso::error::SdkError::service_error(
        service,
        Response::new(StatusCode::try_from(401).unwrap(), SdkBody::empty()),
    );
    let credentials = CredentialsError::provider_error(error);
    assert_eq!(sso_credential_hint(&credentials), Some(SSO_UNAVAILABLE));
}
