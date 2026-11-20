use super::*;

/// Serves one canned HTTP response per connection and returns the request lines it saw.
async fn mock(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut seen = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            seen.push(request.lines().next().unwrap_or_default().to_string());
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = stream.shutdown().await;
        }
        seen
    });
    (base, handle)
}

fn json(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

// TEST-NET-3: never reachable, so a test that wrongly sends to it fails instead of passing.
const REMOTE_HTTP: &str = "http://203.0.113.1/token";

fn err(result: Result<impl Sized, FmError>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(FmError::Other(message)) => message,
        Err(other) => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn only_https_or_loopback_http_is_secure() {
    for ok in [
        "https://login.example.com",
        "http://127.0.0.1:8080/realms/x",
        "http://localhost/",
        "http://[::1]:9000",
    ] {
        assert!(secure_url(ok, "issuer").is_ok(), "{ok}");
    }
    for bad in [
        "http://login.example.com",
        "http://127.0.0.1.evil.example",
        "http://10.0.0.1",
        "ftp://login.example.com",
        "not a url",
    ] {
        assert!(secure_url(bad, "issuer").is_err(), "{bad}");
    }
}

#[tokio::test]
async fn insecure_issuer_and_token_endpoints_are_refused_before_sending() {
    assert!(err(start_auth("http://203.0.113.1", "client", None).await).contains("HTTPS"));
    assert!(err(exchange_code(
        REMOTE_HTTP,
        "code",
        "client",
        "verifier",
        "http://127.0.0.1/cb"
    )
    .await)
    .contains("HTTPS"));
    assert!(err(refresh_tokens(REMOTE_HTTP, "refresh", "client").await).contains("HTTPS"));
}

#[tokio::test]
async fn discovery_returning_http_token_endpoint_is_refused() {
    let (issuer, server) = mock(vec![json(&format!(
        r#"{{"authorization_endpoint":"https://login.example.com/auth","token_endpoint":"{REMOTE_HTTP}"}}"#
    ))])
    .await;
    assert!(err(start_auth(&issuer, "client", None).await)
        .contains("token endpoint URL must use HTTPS"));
    assert_eq!(
        server.await.unwrap(),
        ["GET /.well-known/openid-configuration HTTP/1.1"]
    );
}

#[tokio::test]
async fn redirect_to_insecure_url_is_not_followed() {
    let (base, server) = mock(vec![format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {REMOTE_HTTP}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )])
    .await;
    let message = err(exchange_code(
        &format!("{base}/token"),
        "code",
        "client",
        "verifier",
        "http://127.0.0.1/cb",
    )
    .await);
    assert!(message.contains("error following redirect"), "{message}");
    assert_eq!(server.await.unwrap(), ["POST /token HTTP/1.1"]);
}

#[tokio::test]
async fn secure_flow_still_works() {
    let (token_base, token_server) =
        mock(vec![json(r#"{"id_token":"id","refresh_token":"r"}"#)]).await;
    let (issuer, discovery_server) = mock(vec![json(&format!(
        r#"{{"authorization_endpoint":"{token_base}/auth","token_endpoint":"{token_base}/token"}}"#
    ))])
    .await;
    let session = start_auth(&format!("{issuer}/"), "client", None)
        .await
        .unwrap();
    assert!(session.auth_url.starts_with(&format!("{token_base}/auth?")));
    let tokens = exchange_code(
        &session.token_endpoint,
        "code",
        "client",
        &session.verifier,
        &session.redirect_uri,
    )
    .await
    .unwrap();
    assert_eq!(
        (tokens.id_token.as_str(), tokens.refresh_token.as_deref()),
        ("id", Some("r"))
    );
    assert_eq!(
        discovery_server.await.unwrap(),
        ["GET /.well-known/openid-configuration HTTP/1.1"]
    );
    assert_eq!(token_server.await.unwrap(), ["POST /token HTTP/1.1"]);
}
