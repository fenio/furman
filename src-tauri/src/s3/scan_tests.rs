use super::{listing_tests::service, scan::*, verification::object_sha256_bounded};
use crate::models::{FmError, SearchEvent};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

const ONE: &str = "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>prefix/a.txt</Key><Size>3</Size><ETag>opaque</ETag></Contents></ListBucketResult>";
const TWO: &str = "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>prefix/a.txt</Key><Size>3</Size></Contents><Contents><Key>prefix/b.txt</Key><Size>4</Size></Contents></ListBucketResult>";
const MORE: &str = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>next</NextContinuationToken><Contents><Key>prefix/a.txt</Key><Size>3</Size></Contents></ListBucketResult>";

fn limits(objects: u64, requests: u64) -> ScanLimits {
    ScanLimits {
        max_objects: objects,
        max_requests: requests,
        ..ScanLimits::DEFAULT
    }
}

#[test]
fn scan_wrappers_do_not_embed_large_child_futures() {
    fn large_future() -> impl std::future::Future<Output = Result<(), FmError>> {
        let payload = [0u8; 64 * 1024];
        async move {
            let result = std::future::pending::<Result<(), FmError>>().await;
            std::hint::black_box(payload);
            result
        }
    }
    let budget = ScanBudget::new();
    let cancel = AtomicBool::new(false);
    let waiting = budget.wait(&cancel, large_future());
    let requesting = budget.request(&cancel, large_future());
    assert!(
        std::mem::size_of_val(&waiting) < 1024,
        "wait future is {} bytes",
        std::mem::size_of_val(&waiting)
    );
    assert!(
        std::mem::size_of_val(&requesting) < 1024,
        "request future is {} bytes",
        std::mem::size_of_val(&requesting)
    );
}

#[test]
fn concurrent_budget_reservations_do_not_overshoot_the_limit() {
    let budget = Arc::new(ScanBudget::with_limits(limits(101, 1000)));
    let accepted = Arc::new(std::sync::atomic::AtomicU64::new(0));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let budget = budget.clone();
            let accepted = accepted.clone();
            scope.spawn(move || {
                for _ in 0..100 {
                    if budget.object(&AtomicBool::new(false)).is_ok() {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    assert_eq!(accepted.load(Ordering::Relaxed), 101);
    assert_eq!(budget.progress().scanned, 101);
}

#[test]
fn invalid_limits_cannot_disable_safeguards() {
    for value in [0, 10_000_001] {
        assert!(set_limits(ScanLimits {
            max_objects: value,
            ..ScanLimits::DEFAULT
        })
        .is_err());
    }
    assert!(set_limits(ScanLimits {
        max_seconds: 0,
        ..ScanLimits::DEFAULT
    })
    .is_err());
    assert!(set_limits(ScanLimits {
        max_results: 10_001,
        ..ScanLimits::DEFAULT
    })
    .is_err());
    assert!(set_limits(ScanLimits {
        max_requests: 0,
        ..ScanLimits::DEFAULT
    })
    .is_err());
    assert!(ScanLimits::DEFAULT.validate().is_ok());
}

#[tokio::test]
async fn budgets_are_shared_across_prefixes_and_partial_collections_fail() {
    let (svc, http) = service(&[ONE, TWO]);
    let budget = ScanBudget::with_limits(limits(2, 10));
    let cancel = AtomicBool::new(false);
    let first = collect_objects(&svc.client, "bucket", "first/", &budget, &cancel, &|_| {})
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].etag.as_deref(), Some("opaque"));
    let second = collect_objects(&svc.client, "bucket", "second/", &budget, &cancel, &|_| {}).await;
    assert!(matches!(second, Err(FmError::ScanLimited(_))));
    assert_eq!(budget.progress().scanned, 2);
    assert_eq!(http.actual_requests().count(), 2);
}

#[tokio::test]
async fn request_budget_prevents_the_next_list_call() {
    let (svc, http) = service(&[MORE]);
    let budget = ScanBudget::with_limits(limits(10, 1));
    let result = collect_objects(
        &svc.client,
        "bucket",
        "",
        &budget,
        &AtomicBool::new(false),
        &|_| {},
    )
    .await;
    assert!(matches!(result, Err(FmError::ScanLimited(_))));
    assert_eq!(budget.progress().requests, 1);
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn exact_object_limit_only_succeeds_for_a_complete_scope() {
    for (xml, complete) in [(ONE, true), (MORE, false), (TWO, false)] {
        let (svc, http) = service(&[xml]);
        let budget = ScanBudget::with_limits(limits(1, 10));
        let result = collect_objects(
            &svc.client,
            "bucket",
            "",
            &budget,
            &AtomicBool::new(false),
            &|_| {},
        )
        .await;
        assert_eq!(result.is_ok(), complete);
        assert_eq!(budget.progress().scanned, 1);
        assert_eq!(http.actual_requests().count(), 1);
    }
}

#[tokio::test]
async fn result_limit_is_not_misreported_as_an_exhaustive_search() {
    for (xml, expected) in [
        (ONE, ScanOutcome::Complete),
        (TWO, ScanOutcome::Limited),
        (MORE, ScanOutcome::Limited),
    ] {
        let (svc, http) = service(&[xml]);
        let budget = ScanBudget::with_limits(ScanLimits {
            max_results: 1,
            ..limits(10, 10)
        });
        let result = walk_objects(
            &svc.client,
            "bucket",
            "",
            &budget,
            &AtomicBool::new(false),
            |_| Ok(budget.matched()),
            &|_| {},
        )
        .await;
        assert_eq!(outcome(&result), expected);
        assert_eq!(budget.progress().matched, 1);
        assert_eq!(http.actual_requests().count(), 1);
    }
}

#[tokio::test]
async fn missing_empty_and_cyclic_cursors_fail_instead_of_returning_partial_success() {
    let a = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>a</NextContinuationToken></ListBucketResult>";
    let b = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>b</NextContinuationToken></ListBucketResult>";
    for pages in [
        vec!["<ListBucketResult><IsTruncated>true</IsTruncated></ListBucketResult>"],
        vec!["<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken></NextContinuationToken></ListBucketResult>"],
        vec![a, b, a],
    ] {
        let (svc, _) = service(&pages);
        let result = collect_objects(&svc.client, "bucket", "", &ScanBudget::new(), &AtomicBool::new(false), &|_| {}).await;
        assert!(result.err().unwrap().to_string().contains("continuation token"));
    }
}

#[tokio::test]
async fn stalled_async_reads_obey_the_absolute_deadline() {
    let budget = ScanBudget::with_limits(ScanLimits {
        max_seconds: 1,
        ..ScanLimits::DEFAULT
    });
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        budget.request(
            &AtomicBool::new(false),
            std::future::pending::<Result<(), FmError>>(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(outcome(&result), ScanOutcome::Limited);
    assert_eq!(budget.progress().requests, 1);
}

#[tokio::test]
async fn cancellation_interrupts_a_stalled_request_without_waiting_for_timeout() {
    let budget = ScanBudget::new();
    let cancel = AtomicBool::new(false);
    let (_, result) = tokio::join!(
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel.store(true, Ordering::Relaxed);
        },
        async {
            tokio::time::timeout(
                Duration::from_secs(1),
                budget.request(&cancel, std::future::pending::<Result<(), FmError>>()),
            )
            .await
            .unwrap()
        },
    );
    assert_eq!(outcome(&result), ScanOutcome::Cancelled);
}

#[tokio::test]
async fn failed_recursive_delete_preflight_sends_no_delete_requests() {
    let (svc, http) = service(&[MORE, MORE]);
    let result = svc
        .delete_objects(
            &["prefix/".into()],
            "test",
            &AtomicBool::new(false),
            &|_| {},
        )
        .await;
    assert!(result.is_err());
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method() == "GET"));
}

#[tokio::test]
async fn selection_object_limit_fails_delete_before_any_mutation() {
    let (svc, http) = service(&[]);
    let keys = vec!["file".to_owned(); ScanLimits::DEFAULT.max_objects as usize + 1];
    let result = svc
        .delete_objects(&keys, "test", &AtomicBool::new(false), &|_| {})
        .await;
    assert!(matches!(result, Err(FmError::ScanLimited(_))));
    assert_eq!(http.actual_requests().count(), 0);
}

#[tokio::test]
async fn failed_recursive_copy_and_rename_preflights_send_no_writes() {
    let (svc, http) = service(&[MORE, MORE]);
    let result = svc
        .copy_objects_with_checkpoint(
            &svc.client,
            "bucket",
            &["prefix/".into()],
            &svc.client,
            "other",
            "dest/",
            "test",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            &|_| {},
            None,
            None,
            false,
            false,
        )
        .await;
    assert!(result.is_err());
    assert_eq!(http.actual_requests().count(), 2);
    assert!(http
        .actual_requests()
        .all(|request| request.method() == "GET"));

    let empty = "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>";
    let (svc, http) = service(&[empty, MORE, MORE]);
    assert!(svc.rename_object("prefix/", "renamed").await.is_err());
    assert_eq!(http.actual_requests().count(), 3);
    assert!(http
        .actual_requests()
        .all(|request| request.method() == "GET"));
}

#[tokio::test]
async fn cancelled_preflight_has_no_provider_requests_or_mutations() {
    let (svc, http) = service(&[]);
    let result = svc
        .delete_objects(&["prefix/".into()], "test", &AtomicBool::new(true), &|_| {})
        .await;
    assert_eq!(outcome(&result), ScanOutcome::Cancelled);
    assert_eq!(http.actual_requests().count(), 0);
}

#[tokio::test]
async fn failed_download_preflight_creates_no_local_output() {
    let (svc, http) = service(&[MORE, MORE]);
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("not-created");
    assert!(svc
        .download(
            &["prefix/".into()],
            output.to_str().unwrap(),
            "test",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            &|_| {},
            None
        )
        .await
        .is_err());
    assert!(!output.exists());
    assert_eq!(http.actual_requests().count(), 2);
    assert!(http
        .actual_requests()
        .all(|request| request.uri().to_string().contains("list-type=2")));
}

#[tokio::test]
async fn management_listings_reject_missing_markers_without_looping() {
    let (svc, http) =
        service(&["<ListVersionsResult><IsTruncated>true</IsTruncated></ListVersionsResult>"]);
    assert!(svc.list_object_versions("file").await.is_err());
    assert_eq!(http.actual_requests().count(), 1);
    let (svc, http) = service(&[
        "<ListMultipartUploadsResult><IsTruncated>true</IsTruncated></ListMultipartUploadsResult>",
    ]);
    assert!(svc.list_multipart_uploads(None).await.is_err());
    assert_eq!(http.actual_requests().count(), 1);
    let (svc, http) = service(&["<ListInventoryConfigurationsResult><IsTruncated>true</IsTruncated></ListInventoryConfigurationsResult>"]);
    assert!(svc.list_inventory_configurations().await.is_err());
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn successful_search_emits_explicit_progress_and_complete_outcome() {
    let (svc, _) = service(&[ONE]);
    let events = Mutex::new(Vec::new());
    svc.search_objects("", "txt", false, &AtomicBool::new(false), &|event| {
        events.lock().unwrap().push(event)
    })
    .await
    .unwrap();
    let events = events.lock().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, SearchEvent::ScanProgress { .. })));
    assert!(
        matches!(events.last(), Some(SearchEvent::ScanFinished { outcome: ScanOutcome::Complete, progress, .. }) if progress.matched == 1 && progress.scanned == 1)
    );
    assert!(!events
        .iter()
        .any(|event| matches!(event, SearchEvent::Done(_))));
}

#[tokio::test]
async fn capped_and_cancelled_searches_never_emit_complete_outcomes() {
    let xml = format!(
        "<ListBucketResult><IsTruncated>false</IsTruncated>{}</ListBucketResult>",
        "<Contents><Key>prefix/file.txt</Key><Size>3</Size></Contents>"
            .repeat(ScanLimits::DEFAULT.max_results as usize + 1)
    );
    let (svc, _) = service(&[&xml]);
    let events = Mutex::new(Vec::new());
    let result = svc
        .search_objects("", "txt", false, &AtomicBool::new(false), &|event| {
            events.lock().unwrap().push(event)
        })
        .await;
    assert!(matches!(result, Err(FmError::ScanLimited(_))));
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SearchEvent::Result(_)))
            .count(),
        ScanLimits::DEFAULT.max_results as usize
    );
    assert!(matches!(
        events.last(),
        Some(SearchEvent::ScanFinished {
            outcome: ScanOutcome::Limited,
            ..
        })
    ));

    let (svc, http) = service(&[]);
    let events = Mutex::new(Vec::new());
    let result = svc
        .search_objects("", "txt", false, &AtomicBool::new(true), &|event| {
            events.lock().unwrap().push(event)
        })
        .await;
    assert_eq!(outcome(&result), ScanOutcome::Cancelled);
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(SearchEvent::ScanFinished {
            outcome: ScanOutcome::Cancelled,
            ..
        })
    ));
    assert_eq!(http.actual_requests().count(), 0);
}

#[test]
fn marker_pairs_allow_same_key_with_advancing_version_but_reject_cycles() {
    let mut seen = Default::default();
    assert!(paired_cursor(&mut seen, Some("same-key"), Some("v1")).is_ok());
    assert!(paired_cursor(&mut seen, Some("same-key"), Some("v2")).is_ok());
    assert!(paired_cursor(&mut seen, Some("same-key"), Some("v1")).is_err());
    assert!(paired_cursor(&mut seen, None, Some("v3")).is_err());
    assert!(paired_cursor(&mut seen, Some(""), None).is_err());
}

#[tokio::test]
async fn sha256_reports_actual_s3_bytes_read() {
    use aws_smithy_http_client::test_util::capture_request;
    use aws_smithy_types::body::SdkBody;
    let raw = tauri::http::Response::builder()
        .status(200)
        .body(SdkBody::from("abc"))
        .unwrap();
    let (http, _) = capture_request(Some(raw));
    let (svc, _) = service(&[]);
    let client =
        aws_sdk_s3::Client::from_conf(svc.client.config().to_builder().http_client(http).build());
    let budget = ScanBudget::new();
    let digest = object_sha256_bounded(
        &client,
        "bucket",
        "file",
        3,
        None,
        &AtomicBool::new(false),
        Some((&budget, &|_| {})),
    )
    .await
    .unwrap();
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(budget.progress().bytes_read, 3);
    assert_eq!(budget.progress().s3_bytes_read, 3);
    assert_eq!(budget.progress().requests, 1);
}

#[tokio::test]
async fn sha256_deadline_also_covers_a_stalled_response_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let size = socket.read(&mut buffer).await.unwrap();
            if size == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..size]);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc")
            .await
            .unwrap();
        std::future::pending::<()>().await;
    });
    let (svc, _) = service(&[]);
    let client = aws_sdk_s3::Client::from_conf(
        svc.client
            .config()
            .to_builder()
            .endpoint_url(format!("http://{address}"))
            .http_client(aws_smithy_http_client::Builder::new().build_http())
            .build(),
    );
    let budget = ScanBudget::with_limits(ScanLimits {
        max_seconds: 1,
        ..ScanLimits::DEFAULT
    });
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        object_sha256_bounded(
            &client,
            "bucket",
            "file",
            6,
            None,
            &AtomicBool::new(false),
            Some((&budget, &|_| {})),
        ),
    )
    .await;
    server.abort();
    assert!(matches!(result.unwrap(), Err(FmError::ScanLimited(_))));
    assert_eq!(budget.progress().bytes_read, 3);
    assert_eq!(budget.progress().s3_bytes_read, 3);
}

#[tokio::test]
async fn cancellation_during_enumeration_prevents_followup_pages() {
    let (svc, http) = service(&[MORE]);
    let cancel = Arc::new(AtomicBool::new(false));
    let budget = ScanBudget::new();
    let result = walk_objects(
        &svc.client,
        "bucket",
        "",
        &budget,
        &cancel,
        |_| {
            cancel.store(true, Ordering::Relaxed);
            Ok(false)
        },
        &|_| {},
    )
    .await;
    assert_eq!(outcome(&result), ScanOutcome::Cancelled);
    assert_eq!(http.actual_requests().count(), 1);
}
