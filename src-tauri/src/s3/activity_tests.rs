use super::*;
use aws_sdk_s3::{
    config::{retry::RetryConfig, Region},
    Client, Config,
};
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use tauri::http::{Request, Response};

fn client(responses: &[(u16, &str)], retries: RetryConfig) -> (Client, Arc<Mutex<History>>) {
    let history = Arc::new(Mutex::new(History::default()));
    let http = StaticReplayClient::new(
        responses
            .iter()
            .map(|(status, body)| {
                ReplayEvent::new(
                    Request::builder()
                        .uri("https://activity.test/private-bucket/secret-key")
                        .body(SdkBody::empty())
                        .unwrap(),
                    Response::builder()
                        .status(*status)
                        .header("x-amz-request-id", "request-123")
                        .header("x-secret", "SECRET_RESPONSE_HEADER")
                        .body(SdkBody::from(body.to_string()))
                        .unwrap(),
                )
            })
            .collect(),
    );
    let client = Client::from_conf(
        Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-east-1"))
            .endpoint_url("https://activity.test")
            .force_path_style(true)
            .credentials_provider(aws_credential_types::Credentials::new(
                "SECRET_ACCESS_KEY",
                "SECRET_CREDENTIAL",
                Some("SECRET_TOKEN".into()),
                None,
                "test",
            ))
            .retry_config(retries)
            .http_client(http)
            .interceptor(ActivityInterceptor {
                history: history.clone(),
            })
            .build(),
    );
    (client, history)
}

#[tokio::test]
async fn successful_and_rejected_requests_record_only_safe_structured_fields() {
    let (client, history) = client(&[(200, "SECRET_PAYLOAD"), (403, "<Error><Code>AccessDenied</Code><Message>SECRET_PROVIDER_MESSAGE</Message></Error>")], RetryConfig::disabled());
    let response = client
        .get_object()
        .bucket("private-bucket")
        .key("secret-key")
        .send()
        .await
        .unwrap();
    response.body.collect().await.unwrap();
    assert!(client
        .get_object()
        .bucket("private-bucket")
        .key("secret-key")
        .send()
        .await
        .is_err());
    let history = history.lock().unwrap();
    assert_eq!(history.records.len(), 2);
    let success = &history.records[0];
    let failed = &history.records[1];
    assert_eq!(success.operation, "GetObject");
    assert_eq!(success.outcome, "success");
    assert_eq!(success.http_status, Some(200));
    assert_eq!(success.attempts, 1);
    assert_eq!(failed.outcome, "failed");
    assert_eq!(failed.http_status, Some(403));
    assert_eq!(failed.request_id.as_deref(), Some("request-123"));
    let json = serde_json::to_string(&history.records).unwrap();
    for secret in [
        "SECRET",
        "private-bucket",
        "secret-key",
        "activity.test",
        "Authorization",
        "AccessDenied",
    ] {
        assert!(
            !json.contains(secret),
            "Unexpected sensitive field in activity export: {secret}"
        );
    }
}

#[tokio::test]
async fn retries_are_counted_without_duplicating_logical_operations() {
    let (client, history) = client(
        &[
            (503, "<Error><Code>SlowDown</Code></Error>"),
            (200, "payload"),
        ],
        RetryConfig::standard()
            .with_max_attempts(2)
            .with_initial_backoff(std::time::Duration::ZERO),
    );
    client
        .get_object()
        .bucket("bucket")
        .key("key")
        .send()
        .await
        .unwrap();
    let history = history.lock().unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].attempts, 2);
    assert_eq!(history.records[0].retries, 1);
    assert_eq!(history.records[0].outcome, "success");
}

fn trace(history: Arc<Mutex<History>>) -> Trace {
    let generation = history.lock().unwrap().generation;
    Trace {
        history,
        generation,
        started: Instant::now(),
        started_ms: 0,
        operation: Mutex::new("PutObject".into()),
        attempts: AtomicU32::new(1),
        finished: AtomicBool::new(false),
    }
}

#[test]
fn ring_eviction_is_bounded_and_finished_traces_are_not_duplicated_on_drop() {
    let history = Arc::new(Mutex::new(History::default()));
    for _ in 0..CAPACITY + 5 {
        trace(history.clone()).finish("success", Some(200), None);
    }
    let history = history.lock().unwrap();
    assert_eq!(history.records.len(), CAPACITY);
    assert_eq!(history.evicted, 5);
    assert_eq!(history.records.front().unwrap().id, 6);
    assert_eq!(history.records.back().unwrap().id, (CAPACITY + 5) as u64);
}

#[test]
fn clearing_drops_pending_old_completions_and_abandoned_traces_remain_visible() {
    let history = Arc::new(Mutex::new(History::default()));
    let old = trace(history.clone());
    history.lock().unwrap().clear();
    old.finish("success", Some(200), None);
    drop(old);
    assert!(history.lock().unwrap().records.is_empty());
    drop(trace(history.clone()));
    assert_eq!(history.lock().unwrap().records[0].outcome, "abandoned");
}

#[test]
fn provider_ids_are_bounded_and_reject_arbitrary_text() {
    assert_eq!(
        identifier("request-123/abc==").as_deref(),
        Some("request-123/abc==")
    );
    for invalid in [
        "",
        "<html>secret</html>",
        "a\nb",
        "request with spaces",
        &"x".repeat(129),
    ] {
        assert!(identifier(invalid).is_none());
    }
}

#[tokio::test]
async fn dropping_a_running_sdk_future_records_an_uncertain_abandoned_call() {
    use tokio::{io::AsyncReadExt, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (connected, waiting) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0u8; 16];
        socket.read_exact(&mut bytes).await.unwrap();
        connected.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(socket);
    });
    let (client, history) = client(&[], RetryConfig::disabled());
    let client = Client::from_conf(
        client
            .config()
            .to_builder()
            .endpoint_url(format!("http://{address}"))
            .http_client(aws_smithy_http_client::Builder::new().build_http())
            .build(),
    );
    let task =
        tokio::spawn(async move { client.get_object().bucket("bucket").key("key").send().await });
    tokio::time::timeout(std::time::Duration::from_secs(3), waiting)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    let _ = task.await;
    server.abort();
    let history = history.lock().unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].outcome, "abandoned");
}

#[tokio::test]
async fn sse_c_and_single_attempt_client_clones_preserve_observation_without_recording_keys() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (client, history) = client(&[(200, "")], RetryConfig::disabled());
    let key = STANDARD.encode([42u8; 32]);
    let encrypted = super::super::sse_c::configure(client, Some(&key)).unwrap();
    let cloned = Client::from_conf(
        encrypted
            .config()
            .to_builder()
            .retry_config(RetryConfig::disabled())
            .build(),
    );
    cloned
        .put_object()
        .bucket("private-bucket")
        .key("secret-key")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(
            b"SECRET_PAYLOAD",
        ))
        .send()
        .await
        .unwrap();
    let history = history.lock().unwrap();
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].operation, "PutObject");
    assert_eq!(history.records[0].attempts, 1);
    let json = serde_json::to_string(&history.records).unwrap();
    assert!(!json.contains(&key));
    assert!(!json.contains("SECRET"));
    assert!(!json.contains("private-bucket"));
}
