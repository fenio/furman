use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, Paths) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().canonicalize().unwrap();
    let paths = Paths {
        config: directory.join("config.json"),
        store: directory.join("saved-jobs.json"),
    };
    let config = serde_json::json!({"theme":"dark", "connections":[profile("source", "bucket-a"), profile("destination", "bucket-b")]});
    fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
    (root, paths)
}
fn profile(id: &str, bucket: &str) -> Value {
    serde_json::json!({"id":id,"name":id,"type":"s3","credentialType":"keychain","accessKeyId":"DO_NOT_PERSIST_ACCESS_ID","bucket":bucket,"region":"us-east-1","provider":"linode","endpoint":"https://us-ord-1.linodeobjects.com"})
}
fn request(paths: &Paths, id: Option<String>) -> SaveRequest {
    SaveRequest {
        expected_revision: list(paths).unwrap().revision,
        expected_configuration_revision: list(paths).unwrap().configuration_revision,
        id,
        name: "Migration content check".into(),
        source: SourceRequest::S3 {
            profile_id: "source".into(),
            prefix: "source//literal/../資料/".into(),
        },
        destination_profile_id: "destination".into(),
        destination_prefix: "target/".into(),
        exclude_patterns: vec![],
        acknowledged: true,
    }
}
fn job(paths: &Paths) -> Job {
    list(paths).unwrap().jobs[0].definition.clone()
}

#[test]
fn definitions_roundtrip_exact_pins_without_credentials_resolution_or_execution() {
    let (_root, paths) = fixture();
    assert!(!paths.store.exists());
    let empty = list(&paths).unwrap();
    assert!(empty.jobs.is_empty());
    assert!(!empty.execution_available);
    assert!(!paths.store.exists());
    let report = save(&paths, request(&paths, None)).unwrap();
    assert_eq!(report.jobs[0].status, "definition_valid");
    assert!(!report.execution_available);
    let source = &report.jobs[0].definition.source;
    let SourcePin::S3 { pin } = source else {
        panic!("S3 source expected")
    };
    assert_eq!(pin.prefix, "source//literal/../資料/");
    assert_eq!(pin.profile_id, "source");
    assert!(report.jobs[0].definition.exclude_patterns.is_empty());
    let bytes = fs::read(&paths.store).unwrap();
    let encoded = String::from_utf8(bytes).unwrap();
    assert!(!encoded.contains("DO_NOT_PERSIST_ACCESS_ID"));
    assert!(!encoded.contains("accessKeyId"));
    assert!(!encoded.contains("secret"));
    assert_eq!(
        fs::metadata(&paths.store).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(matches!(
        report.jobs[0].definition.compare_mode,
        JobMode::Sha256
    ));
}

#[test]
fn managed_secret_changes_invalidate_before_mutation_and_fail_closed_on_failure() {
    let (_root, paths) = fixture();
    save(&paths, request(&paths, None)).unwrap();
    let original = job(&paths).destination.generation;
    let result: Result<(), FmError> = secret_edit(&paths, "destination:sse-c", || {
        assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
        assert!(!load(&paths).unwrap().1.generations["destination"].ready);
        Err(error("Simulated keychain edit failure"))
    });
    assert!(result.is_err());
    assert!(!load(&paths).unwrap().1.generations["destination"].ready);
    assert!(save(&paths, request(&paths, Some(job(&paths).id))).is_err());
    secret_edit(&paths, "destination:sse-c", || Ok(())).unwrap();
    assert_ne!(
        load(&paths).unwrap().1.generations["destination"].value,
        original
    );
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
    save(&paths, request(&paths, Some(job(&paths).id))).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "definition_valid");
}

#[test]
fn managed_configuration_edits_track_credentials_and_transport_but_not_display_preferences() {
    let (_root, paths) = fixture();
    save(&paths, request(&paths, None)).unwrap();
    let mut config: Value = serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
    let original = job(&paths);
    let generation = original.destination.generation.clone();
    config["theme"] = "light".into();
    config["connections"][1]["name"] = "Renamed display".into();
    write_config(&paths, &serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "definition_valid");
    assert_eq!(
        load(&paths).unwrap().1.generations["destination"].value,
        generation
    );
    config["connections"][1]["accessKeyId"] = "CHANGED_ACCESS_ID_NOT_PERSISTED".into();
    write_config(&paths, &serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
    assert!(!String::from_utf8(fs::read(&paths.store).unwrap())
        .unwrap()
        .contains("CHANGED_ACCESS_ID_NOT_PERSISTED"));
    save(&paths, request(&paths, Some(original.id.clone()))).unwrap();
    config["connections"][1]["endpoint"] = "https://another.example".into();
    write_config(&paths, &serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
    config["connections"] = serde_json::json!([profile("source", "bucket-a")]);
    write_config(&paths, &serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
}

#[test]
fn local_pins_use_no_follow_directory_identity_and_detect_replacements() {
    let (root, paths) = fixture();
    let source = root.path().canonicalize().unwrap().join("source-dir");
    fs::create_dir(&source).unwrap();
    let mut create = request(&paths, None);
    create.source = SourceRequest::Local {
        path: source.to_str().unwrap().into(),
    };
    save(&paths, create).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "definition_valid");
    fs::rename(&source, source.with_extension("old")).unwrap();
    fs::create_dir(&source).unwrap();
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
    let link = source.with_extension("link");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(capture_local(link.to_str().unwrap()).is_err());
    assert!(capture_local(&format!("{}/../", source.display())).is_err());
    let fifo = source.join("fifo");
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
    assert!(capture_local(fifo.to_str().unwrap()).is_err());
    assert!(capture_local("relative").is_err());
}

#[test]
fn profile_eligibility_never_exports_credential_bearing_urls_or_dynamic_auth() {
    let base = profile("p", "bucket");
    let mut missing_endpoint = base.clone();
    missing_endpoint["endpoint"] = Value::Null;
    assert!(configuration(&missing_endpoint).is_err());
    missing_endpoint["provider"] = "aws".into();
    assert!(configuration(&missing_endpoint).unwrap().endpoint.is_none());
    for endpoint in [
        "https://user:SECRET@example.com",
        "https://@example.com",
        "https://example.com?token=SECRET",
        "https://example.com/#SECRET",
        "https://example.com/path",
        "https://example.com/a/..",
        "https://example.com/%2e/",
    ] {
        let mut value = base.clone();
        value["endpoint"] = endpoint.into();
        let failure = configuration(&value).err().unwrap().to_string();
        assert!(!failure.contains("SECRET"));
    }
    assert_eq!(
        safe_url("HTTPS://EXAMPLE.COM:443").unwrap(),
        "https://example.com/"
    );
    for credential_type in ["default", "aws-profile", "anonymous", "oidc"] {
        let mut value = base.clone();
        value["credentialType"] = credential_type.into();
        assert!(configuration(&value).is_err());
    }
    for key in ["roleArn", "profile", "externalId", "oidcIssuerUrl"] {
        let mut value = base.clone();
        value[key] = "not-supported".into();
        assert!(configuration(&value).is_err());
    }
    let mut value = base.clone();
    value["proxyUrl"] = "system".into();
    assert!(configuration(&value).is_err());
    let mut value = base;
    value["sseCustomerKeyEnabled"] = true.into();
    value["endpoint"] = "http://example.com".into();
    assert!(configuration(&value).is_err());
}

#[test]
fn namespace_overlap_uses_native_resolved_endpoint_bucket_and_literal_folder_prefixes() {
    let (_root, paths) = fixture();
    let mut config: Value = serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
    config["connections"][1]["bucket"] = "bucket-a".into();
    fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
    for dest in ["", "source/", "source/child/"] {
        let mut create = request(&paths, None);
        create.source = SourceRequest::S3 {
            profile_id: "source".into(),
            prefix: "source/".into(),
        };
        create.destination_prefix = dest.into();
        assert!(save(&paths, create).is_err());
    }
    let mut create = request(&paths, None);
    create.source = SourceRequest::S3 {
        profile_id: "source".into(),
        prefix: "source/".into(),
    };
    create.destination_prefix = "source-adjacent/".into();
    save(&paths, create).unwrap();
    let mut create = request(&paths, None);
    create.name = "No implicit trailing slash".into();
    create.destination_prefix = "stem".into();
    assert!(save(&paths, create).is_err());
}

#[test]
fn stale_rebind_delete_and_unacknowledged_operations_do_not_write() {
    let (_root, paths) = fixture();
    let stale = request(&paths, None);
    let first = save(&paths, request(&paths, None)).unwrap();
    let before = fs::read(&paths.store).unwrap();
    assert!(save(&paths, stale).is_err());
    assert_eq!(fs::read(&paths.store).unwrap(), before);
    assert!(delete(&paths, &first.revision, &first.jobs[0].definition.id, false).is_err());
    let id = job(&paths).id;
    let original_revision = job(&paths).revision;
    let mut rebind = request(&paths, Some(id.clone()));
    rebind.acknowledged = false;
    assert!(save(&paths, rebind).is_err());
    save(&paths, request(&paths, Some(id.clone()))).unwrap();
    assert_ne!(job(&paths).revision, original_revision);
    assert!(delete(&paths, &first.revision, &id, true).is_err());
    let result = delete(&paths, &list(&paths).unwrap().revision, &id, true).unwrap();
    assert!(result.jobs.is_empty());
}

#[test]
fn corrupt_or_future_store_blocks_managed_edits_without_discarding_data() {
    let (_root, paths) = fixture();
    for content in [
        "INVALID PRIVATE_BODY",
        r#"{"version":2,"generations":{},"jobs":[]}"#,
        r#"{"version":1,"generations":{},"jobs":[],"secret":"PRIVATE_BODY"}"#,
    ] {
        fs::write(&paths.store, content).unwrap();
        let failure = list(&paths).err().unwrap().to_string();
        assert!(!failure.contains("PRIVATE_BODY"));
        let mut changed = false;
        assert!(secret_edit(&paths, "destination", || {
            changed = true;
            Ok(())
        })
        .is_err());
        assert!(!changed);
        assert_eq!(fs::read_to_string(&paths.store).unwrap(), content);
    }
}

#[test]
fn storage_bounds_symlinks_lock_contention_and_failed_config_edits_are_visible() {
    let (_root, paths) = fixture();
    let guard = lock(&paths).unwrap();
    assert!(save(&paths, request(&paths, None)).is_err());
    drop(guard);
    save(&paths, request(&paths, None)).unwrap();
    let bytes = fs::read(&paths.store).unwrap();
    fs::remove_file(&paths.store).unwrap();
    let target = paths.store.with_extension("target");
    fs::write(&target, &bytes).unwrap();
    std::os::unix::fs::symlink(&target, &paths.store).unwrap();
    assert!(list(&paths).is_err());
    fs::remove_file(&paths.store).unwrap();
    fs::File::create(&paths.store)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(list(&paths).is_err());
    fs::write(&paths.store, bytes).unwrap();
    let before = fs::read(&paths.config).unwrap();
    assert!(write_config(&paths, "invalid PRIVATE_BODY").is_err());
    assert_eq!(fs::read(&paths.config).unwrap(), before);
    // Arbitrary legacy credential IDs are outside the job schema, not prohibited ordinarily.
    let mut ordinary = false;
    secret_edit(&paths, "legacy:id:with:colons", || {
        ordinary = true;
        Ok(())
    })
    .unwrap();
    assert!(ordinary);
}

#[test]
fn schemas_reject_synthetic_configuration_credentials_modes_and_approvals() {
    let (_root, paths) = fixture();
    save(&paths, request(&paths, None)).unwrap();
    let base = serde_json::to_value(job(&paths)).unwrap();
    for key in [
        "secret_key",
        "approval",
        "write_policy",
        "connection_id",
        "schedule",
    ] {
        let mut value = base.clone();
        value[key] = "REJECT".into();
        assert!(serde_json::from_value::<Job>(value).is_err());
    }
    for (key, value) in [("compare_mode", "size_mtime"), ("operation", "delete")] {
        let mut changed = base.clone();
        changed[key] = value.into();
        assert!(serde_json::from_value::<Job>(changed).is_err());
    }
    let mut value = serde_json::json!({"expected_revision":"missing","id":null,"name":"Name","source":{"kind":"s3","profile_id":"source","prefix":""},"destination_profile_id":"destination","destination_prefix":"","exclude_patterns":[],"acknowledged":true});
    value["source"]["configuration"] = serde_json::json!({"endpoint":"https://attacker.example"});
    assert!(serde_json::from_value::<SaveRequest>(value).is_err());
}

#[test]
fn live_connection_generations_are_native_per_instance_not_profile_or_principal_claims() {
    let sdk = aws_config::SdkConfig::builder()
        .behavior_version(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .build();
    let client = aws_sdk_s3::Client::from_conf(aws_sdk_s3::config::Builder::from(&sdk).build());
    let first = crate::s3::S3Connection::new(
        client.clone(),
        "bucket".into(),
        "us-east-1".into(),
        sdk.clone(),
    );
    let second = crate::s3::S3Connection::new(client, "bucket".into(), "us-east-1".into(), sdk);
    assert_ne!(first.instance_id(), second.instance_id());
    assert!(uuid::Uuid::parse_str(first.instance_id()).is_ok());
}

#[test]
fn unmanaged_namespace_changes_cannot_be_silently_captured_after_review() {
    let (_root, paths) = fixture();
    let reviewed = request(&paths, None);
    let mut config: Value = serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
    config["connections"][1]["endpoint"] = "https://different.example".into();
    fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(save(&paths, reviewed)
        .err()
        .unwrap()
        .to_string()
        .contains("changed since review"));
    assert!(!paths.store.exists());
}

#[test]
fn bounded_names_patterns_job_counts_and_duplicate_profile_ids_fail_closed() {
    let (_root, paths) = fixture();
    for name in ["", " surrounding ", "bad\nname"] {
        let mut create = request(&paths, None);
        create.name = name.into();
        assert!(save(&paths, create).is_err());
    }
    for exclusions in [
        vec!["".into()],
        vec!["with,comma".into()],
        vec!["x".repeat(513)],
        vec!["*.tmp".into(); 129],
    ] {
        let mut create = request(&paths, None);
        create.exclude_patterns = exclusions;
        assert!(save(&paths, create).is_err());
    }
    save(&paths, request(&paths, None)).unwrap();
    let mut duplicate = request(&paths, None);
    duplicate.name = job(&paths).name.to_uppercase();
    assert!(save(&paths, duplicate).is_err());
    let (_, mut store) = load(&paths).unwrap();
    let first = store.jobs[0].clone();
    store.jobs = (0..101)
        .map(|index| {
            let mut copy = first.clone();
            copy.id = uuid();
            copy.name = format!("job {index}");
            copy
        })
        .collect();
    assert!(validate_store(&store).is_err());
    fs::write(&paths.config, serde_json::to_vec(&serde_json::json!({"connections":[profile("duplicate", "a"), profile("duplicate", "b")]})).unwrap()).unwrap();
    assert!(list(&paths).is_err());
}

#[test]
fn failed_generation_commit_never_mutates_credentials_and_failed_completion_stays_blocked() {
    let (_root, paths) = fixture();
    save(&paths, request(&paths, None)).unwrap();
    // A corrupted store cannot authorize a secret mutation.
    let original = fs::read(&paths.store).unwrap();
    fs::write(&paths.store, "invalid").unwrap();
    let mut called = false;
    assert!(secret_edit(&paths, "destination:proxy", || {
        called = true;
        Ok(())
    })
    .is_err());
    assert!(!called);
    fs::write(&paths.store, original).unwrap();
    // Simulate storage becoming unavailable after the side effect. The durable
    // pre-edit document retained in backup must still contain a pending marker.
    let backup = paths.store.with_extension("pending-backup");
    let result = secret_edit(&paths, "destination:proxy", || {
        fs::rename(&paths.store, &backup).unwrap();
        fs::create_dir(&paths.store).unwrap();
        Ok(())
    });
    assert!(result.is_err());
    assert!(list(&paths).is_err());
    fs::remove_dir(&paths.store).unwrap();
    fs::rename(backup, &paths.store).unwrap();
    assert!(!load(&paths).unwrap().1.generations["destination"].ready);
    assert_eq!(list(&paths).unwrap().jobs[0].status, "needs_rebind");
}
