//! Actual SFTP protocol over private duplex streams: no credentials, SSH trust
//! changes, external server, filesystem writes or production connections.
use super::*;
use russh_sftp::protocol::{Attrs, Data, File, Handle, Name, OpenFlags, Status, StatusCode};
use std::sync::{Arc, Mutex};

struct Server {
    calls: Arc<Mutex<Vec<String>>>,
    mode: &'static str,
    listed: HashSet<String>,
    fstats: usize,
}

fn attr(directory: bool) -> FileAttributes {
    FileAttributes {
        size: Some(if directory { 0 } else { 3 }),
        mtime: Some(1),
        permissions: Some(if directory { 0o040755 } else { 0o100644 }),
        ..Default::default()
    }
}

impl russh_sftp::server::Handler for Server {
    type Error = StatusCode;
    fn unimplemented(&self) -> Self::Error {
        self.calls.lock().unwrap().push("UNEXPECTED".into());
        StatusCode::OpUnsupported
    }
    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.calls.lock().unwrap().push(format!("lstat:{path}"));
        if self.mode == "denied" && path.contains("file") {
            return Err(StatusCode::PermissionDenied);
        }
        let mut attrs = attr(path.ends_with('/') || path.ends_with("nested"));
        if self.mode == "missing-size" && path.ends_with("file") {
            attrs.size = None;
        }
        if self.mode == "link" && path.ends_with("file") {
            attrs.permissions = Some(0o120777);
        }
        Ok(Attrs { id, attrs })
    }
    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        self.calls.lock().unwrap().push(format!("opendir:{path}"));
        self.listed.remove(&path);
        Ok(Handle { id, handle: path })
    }
    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if self.mode == "stall" {
            std::future::pending::<()>().await;
        }
        if !self.listed.insert(handle.clone()) && self.mode != "repeat-page" {
            return Err(StatusCode::Eof);
        }
        let files = if handle == "/" {
            match self.mode {
                "empty-page" => vec![],
                "bad-name" => vec![File::new("../escape", attr(false))],
                "duplicate" => vec![
                    File::new("file", attr(false)),
                    File::new("file", attr(false)),
                ],
                _ => vec![
                    File::new("nested", attr(true)),
                    File::new("file", attr(false)),
                ],
            }
        } else {
            vec![File::new("child", attr(false))]
        };
        Ok(Name { id, files })
    }
    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        assert_eq!(pflags.bits(), OpenFlags::READ.bits());
        self.calls
            .lock()
            .unwrap()
            .push(format!("open-read:{filename}"));
        Ok(Handle {
            id,
            handle: filename,
        })
    }
    async fn fstat(&mut self, id: u32, _handle: String) -> Result<Attrs, Self::Error> {
        self.fstats += 1;
        let mut attrs = attr(false);
        if self.mode == "drift" && self.fstats > 1 {
            attrs.mtime = Some(2);
        }
        Ok(Attrs { id, attrs })
    }
    async fn read(
        &mut self,
        id: u32,
        _handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        self.calls.lock().unwrap().push("read".into());
        let contents: &[u8] = match self.mode {
            "short" => b"ab",
            "long" => b"abcd",
            _ => b"abc",
        };
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        if start >= contents.len() {
            return Err(StatusCode::Eof);
        }
        Ok(Data {
            id,
            data: contents[start..contents.len().min(start + len as usize)].to_vec(),
        })
    }
    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: String::new(),
            language_tag: String::new(),
        })
    }
}

async fn protocol_service(
    mode: &'static str,
) -> (
    Session,
    Arc<Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    let (client, server) = tokio::io::duplex(128 * 1024);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handler = Server {
        calls: calls.clone(),
        mode,
        listed: HashSet::new(),
        fstats: 0,
    };
    let task = tokio::spawn(async move {
        russh_sftp::server::run(server, handler).await;
    });
    let session = Session::new(
        client,
        "fixture".into(),
        22,
        &AtomicBool::new(false),
        &ScanBudget::new(),
    )
    .await
    .unwrap();
    (session, calls, task)
}

#[tokio::test]
async fn recursive_hashes_and_exclusions_use_read_only_protocol_and_exact_paths() {
    for checksum in [false, true] {
        let (service, calls, task) = protocol_service("normal").await;
        let files = collect(
            &service,
            "sftp://fixture:22/",
            checksum,
            &AtomicBool::new(false),
            &[],
            &ScanBudget::new(),
            &|_| {},
        )
        .await
        .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files["nested/child"].0, 3);
        if checksum {
            assert_eq!(
                files["file"].2.as_deref(),
                Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            );
        } else {
            assert!(files["file"].2.is_none());
        }
        let recorded = calls.lock().unwrap().clone();
        assert!(!recorded.iter().any(|call| call == "UNEXPECTED"));
        assert_eq!(
            recorded.iter().any(|call| call.starts_with("open-read:")),
            checksum
        );
        let files = collect(
            &service,
            "sftp://fixture/",
            checksum,
            &AtomicBool::new(false),
            &["nested/**".into(), "file".into()],
            &ScanBudget::new(),
            &|_| {},
        )
        .await
        .unwrap();
        assert!(files.is_empty());
        let files = collect(
            &service,
            "sftp://fixture/",
            checksum,
            &AtomicBool::new(false),
            &["file/**".into()],
            &ScanBudget::new(),
            &|_| {},
        )
        .await
        .unwrap();
        assert!(
            files.contains_key("file"),
            "directory-only exclusions cannot hide a regular file"
        );
        task.abort();
    }
}

#[tokio::test]
async fn authority_mismatch_invalid_entries_read_failures_and_drift_never_return_complete_maps() {
    for mode in [
        "bad-name",
        "empty-page",
        "repeat-page",
        "missing-size",
        "duplicate",
        "link",
        "denied",
        "short",
        "long",
        "drift",
    ] {
        let (service, _, task) = protocol_service(mode).await;
        let result = collect(
            &service,
            "sftp://fixture/",
            true,
            &AtomicBool::new(false),
            &[],
            &ScanBudget::new(),
            &|_| {},
        )
        .await;
        assert!(result.is_err(), "mode {mode} must fail");
        task.abort();
    }
    let (service, calls, task) = protocol_service("normal").await;
    for uri in [
        "sftp://other/",
        "sftp://fixture:23/",
        "sftp://fixture/a/../b",
        "/not-a-uri",
    ] {
        assert!(collect(
            &service,
            uri,
            false,
            &AtomicBool::new(false),
            &[],
            &ScanBudget::new(),
            &|_| {}
        )
        .await
        .is_err());
    }
    assert!(calls.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn budgets_and_cancellation_cover_recursive_metadata_and_stalled_remote_reads() {
    for requests in [false, true] {
        let (service, _, task) = protocol_service("normal").await;
        let limits = crate::s3::scan::ScanLimits {
            max_objects: if requests { 100 } else { 1 },
            max_requests: if requests { 1 } else { 100 },
            ..crate::s3::scan::ScanLimits::DEFAULT
        };
        assert!(matches!(
            collect(
                &service,
                "sftp://fixture/",
                false,
                &AtomicBool::new(false),
                &[],
                &ScanBudget::with_limits(limits),
                &|_| {}
            )
            .await,
            Err(FmError::ScanLimited(_))
        ));
        task.abort();
    }
    let (service, _, task) = protocol_service("stall").await;
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        signal.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        collect(
            &service,
            "sftp://fixture/",
            false,
            &cancel,
            &[],
            &ScanBudget::new(),
            &|_| {},
        ),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(FmError::ScanCancelled)));
    task.abort();
    let (service, _, task) = protocol_service("stall").await;
    let limits = crate::s3::scan::ScanLimits {
        max_seconds: 1,
        ..crate::s3::scan::ScanLimits::DEFAULT
    };
    assert!(matches!(
        collect(
            &service,
            "sftp://fixture/",
            false,
            &AtomicBool::new(false),
            &[],
            &ScanBudget::with_limits(limits),
            &|_| {}
        )
        .await,
        Err(FmError::ScanLimited(_))
    ));
    task.abort();
}

#[tokio::test]
#[ignore = "requires explicit loopback disposable SFTP fixture port and isolated known_hosts"]
async fn loopback_sftp_native_comparison_preserves_payloads_and_the_original_pane_session() {
    use tokio::io::AsyncWriteExt;
    let port = std::env::var("FURMAN_COMPARE_SFTP_TEST_PORT")
        .expect("Set only for a disposable loopback atmoz/sftp testuser:testpass fixture")
        .parse::<u16>()
        .unwrap();
    let connection = crate::sftp::client::build_sftp_client(
        "localhost",
        port,
        "testuser",
        "password",
        Some("testpass"),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let root = format!(
        "{}/upload/{}",
        connection.home_dir.trim_end_matches('/'),
        uuid::Uuid::new_v4()
    );
    connection.session.create_dir(&root).await.unwrap();
    connection
        .session
        .create_dir(format!("{root}/nested"))
        .await
        .unwrap();
    for relative in ["file", "nested/child", "skip.tmp"] {
        let mut file = connection
            .session
            .create(format!("{root}/{relative}"))
            .await
            .unwrap();
        file.write_all(b"abc").await.unwrap();
        file.shutdown().await.unwrap();
    }
    let uri = format!("sftp://localhost:{port}{root}/");
    for checksum in [false, true] {
        let budget = ScanBudget::new();
        let cancel = AtomicBool::new(false);
        let session = Session::from_ssh(
            &connection.ssh_handle,
            "localhost".into(),
            port,
            &cancel,
            &budget,
        )
        .await
        .unwrap();
        let files = collect(
            &session,
            &uri,
            checksum,
            &cancel,
            &["*.tmp".into()],
            &budget,
            &|_| {},
        )
        .await
        .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files["nested/child"].0, 3);
        if checksum {
            assert_eq!(
                files["file"].2.as_deref(),
                Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            );
        }
        drop(session);
        // Dropping the comparison channel must not disconnect pane browsing.
        assert_eq!(
            connection
                .session
                .read(format!("{root}/file"))
                .await
                .unwrap(),
            b"abc"
        );
    }
    let budget = ScanBudget::with_limits(crate::s3::scan::ScanLimits {
        max_objects: 1,
        ..crate::s3::scan::ScanLimits::DEFAULT
    });
    let cancel = AtomicBool::new(false);
    let session = Session::from_ssh(
        &connection.ssh_handle,
        "localhost".into(),
        port,
        &cancel,
        &budget,
    )
    .await
    .unwrap();
    assert!(matches!(
        collect(&session, &uri, true, &cancel, &[], &budget, &|_| {}).await,
        Err(FmError::ScanLimited(_))
    ));
    drop(session);
    for relative in ["file", "nested/child", "skip.tmp"] {
        assert_eq!(
            connection
                .session
                .read(format!("{root}/{relative}"))
                .await
                .unwrap(),
            b"abc"
        );
        connection
            .session
            .remove_file(format!("{root}/{relative}"))
            .await
            .unwrap();
    }
    connection
        .session
        .remove_dir(format!("{root}/nested"))
        .await
        .unwrap();
    connection.session.remove_dir(root).await.unwrap();
}
