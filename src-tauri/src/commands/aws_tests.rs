use super::*;
use std::os::unix::fs::PermissionsExt;

fn script(body: &str) -> (tempfile::TempDir, PathBuf, ProfilePaths) {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("aws");
    std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let paths = ProfilePaths {
        config: root.path().join("config"),
        credentials: root.path().join("credentials"),
    };
    (root, program, paths)
}

#[test]
fn version_validation_never_returns_arbitrary_process_output() {
    assert_eq!(
        parse_version(b"aws-cli/2.42.1 Python SECRET", b"").unwrap(),
        "2.42.1"
    );
    assert_eq!(
        parse_version(b"", b"aws-cli/2.1.0 platform").unwrap(),
        "2.1.0"
    );
    for value in [
        b"aws-cli/1.42.1 SECRET".as_slice(),
        b"aws-cli/2... SECRET",
        b"not aws SECRET",
    ] {
        let error = parse_version(value, b"").err().unwrap().to_string();
        assert!(!error.contains("SECRET"));
    }
}

#[test]
fn login_jobs_are_single_flight_cancellable_and_cleanup_on_drop() {
    let state = AwsSsoState::default();
    let guard = state.begin("job-1").unwrap();
    assert!(state.begin("job-2").is_err());
    assert!(!state.cancel("wrong-job").unwrap());
    assert!(!guard.cancel.load(Ordering::Relaxed));
    assert!(state.cancel("job-1").unwrap());
    assert!(guard.cancel.load(Ordering::Relaxed));
    drop(guard);
    assert!(state.begin("job-2").is_ok());
    assert!(state.begin("../invalid").is_err());
}

#[tokio::test]
async fn profile_membership_and_role_chain_target_are_rechecked() {
    let root = tempfile::tempdir().unwrap();
    let paths = ProfilePaths {
        config: root.path().join("config"),
        credentials: root.path().join("credentials"),
    };
    std::fs::write(&paths.config, "[profile sso]\nsso_start_url=https://example.awsapps.com/start\nsso_region=us-east-1\nsso_account_id=account\nsso_role_name=role\n[profile role]\nrole_arn=arn:role\nsource_profile=sso\n[profile linode]\nregion=us-ord-1\n").unwrap();
    let profiles = aws_profiles::load(&paths).await.unwrap();
    let report = aws_profiles::summarize(&profiles).unwrap();
    resolve_login(&report, "role", "sso").unwrap();
    assert!(resolve_login(&report, "role", "other-session").is_err());
    assert!(resolve_login(&report, "linode", "linode").is_err());
    assert!(resolve_login(&report, "missing", "sso").is_err());
}

#[tokio::test]
async fn sign_in_uses_literal_argv_and_absolute_matching_config_paths() {
    let (root, program, paths) = script("if [ \"$1\" = '--version' ]; then printf 'aws-cli/2.42.1 fake'; exit 0; fi\nprintf '%s\\n' \"$#\" \"$1\" \"$2\" \"$3\" > \"$AWS_CONFIG_FILE.args\"\nprintf '%s\\n' \"$AWS_CONFIG_FILE\" \"$AWS_SHARED_CREDENTIALS_FILE\" > \"$AWS_CONFIG_FILE.paths\"\nprintf 'SECRET_BROWSER_URL_CODE'\nprintf 'SECRET_REFRESH_TOKEN' >&2\nexit 0");
    let profile = "--debug;$(touch SHOULD_NOT_EXIST) 'quoted'";
    let result = login(
        &program,
        &paths,
        profile,
        &AtomicBool::new(false),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    let arguments = std::fs::read_to_string(paths.config.with_extension("args")).unwrap();
    assert_eq!(arguments, format!("3\nsso\nlogin\n--profile={profile}\n"));
    let env = std::fs::read_to_string(paths.config.with_extension("paths")).unwrap();
    assert_eq!(
        env,
        format!(
            "{}\n{}\n",
            paths.config.display(),
            paths.credentials.display()
        )
    );
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("SECRET"));
    assert_eq!(result.cli_version, "2.42.1");
    assert!(!root.path().join("SHOULD_NOT_EXIST").exists());
}

#[tokio::test]
async fn cli_v1_or_invalid_version_never_starts_login() {
    let (_root, program, paths) = script("if [ \"$1\" = '--version' ]; then printf 'aws-cli/1.42.1 SECRET'; exit 0; fi\ntouch \"$AWS_CONFIG_FILE.login\"");
    let error = login(
        &program,
        &paths,
        "sso",
        &AtomicBool::new(false),
        Duration::from_secs(1),
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("v2"));
    assert!(!error.contains("SECRET"));
    assert!(!paths.config.with_extension("login").exists());
}

#[tokio::test]
async fn login_failures_do_not_export_browser_codes_tokens_or_raw_cli_output() {
    let (_root, program, paths) = script("if [ \"$1\" = '--version' ]; then printf 'aws-cli/2.42.1 fake'; exit 0; fi\nprintf 'https://private.example/auth?code=SECRET'\nprintf 'SECRET_TOKEN' >&2\nexit 53");
    let error = login(
        &program,
        &paths,
        "sso",
        &AtomicBool::new(false),
        Duration::from_secs(1),
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("exit 53"));
    assert!(!error.contains("SECRET"));
    assert!(!error.contains("private.example"));
}

#[tokio::test]
async fn deadlines_and_cancellation_stop_cli_without_automatic_relaunch() {
    let (_root, program, paths) = script("if [ \"$1\" = '--version' ]; then printf 'aws-cli/2.42.1 fake'; exit 0; fi\nprintf '%s' \"$$\" > \"$AWS_CONFIG_FILE.pid\"\nexec /bin/sleep 30");
    let error = login(
        &program,
        &paths,
        "sso",
        &AtomicBool::new(false),
        Duration::from_millis(50),
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("exceeded"));
    let pid = std::fs::read_to_string(paths.config.with_extension("pid")).unwrap();
    assert!(!std::process::Command::new("/bin/kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let pid_path = paths.config.with_extension("pid");
    std::fs::remove_file(&pid_path).unwrap();
    let signal = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !pid_path.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        flag.store(true, Ordering::Relaxed);
    });
    let error = login(&program, &paths, "sso", &cancel, Duration::from_secs(3))
        .await
        .err()
        .unwrap()
        .to_string();
    signal.await.unwrap();
    assert!(error.contains("cancelled"));
    let pid = std::fs::read_to_string(paths.config.with_extension("pid")).unwrap();
    assert!(!std::process::Command::new("/bin/kill")
        .args(["-0", &pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn pre_cancelled_jobs_do_not_launch_even_the_version_check() {
    let (_root, program, paths) =
        script("touch \"$AWS_CONFIG_FILE.started\"\nprintf 'aws-cli/2.42.1 fake'");
    assert!(login(
        &program,
        &paths,
        "sso",
        &AtomicBool::new(true),
        Duration::from_secs(1)
    )
    .await
    .is_err());
    assert!(!paths.config.with_extension("started").exists());
}

#[tokio::test]
async fn oversized_version_output_stops_before_login_and_is_not_reported() {
    let (_root, program, paths) = script("if [ \"$1\" = '--version' ]; then i=0; while [ \"$i\" -lt 2048 ]; do printf x; i=$((i+1)); done; exit 0; fi\ntouch \"$AWS_CONFIG_FILE.login\"");
    let error = login(
        &program,
        &paths,
        "sso",
        &AtomicBool::new(false),
        Duration::from_secs(1),
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("safety limit"), "{error}");
    assert!(!paths.config.with_extension("login").exists());
}
