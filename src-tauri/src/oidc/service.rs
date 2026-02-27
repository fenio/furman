use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::models::FmError;

// ── Discovery ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
}

pub struct OidcTokens {
    pub id_token: String,
    pub refresh_token: Option<String>,
}

async fn discover(issuer_url: &str) -> Result<OidcDiscovery, FmError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| FmError::Other(format!("OIDC discovery failed: {e}")))?;
    resp.json::<OidcDiscovery>()
        .await
        .map_err(|e| FmError::Other(format!("OIDC discovery parse failed: {e}")))
}

// ── PKCE ─────────────────────────────────────────────────────────────────────

fn generate_pkce() -> (String, String) {
    let verifier_bytes: [u8; 32] = rand::random();
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

// ── Auth Flow ────────────────────────────────────────────────────────────────

pub struct AuthSession {
    pub auth_url: String,
    pub listener: TcpListener,
    pub verifier: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
}

pub async fn start_auth(
    issuer_url: &str,
    client_id: &str,
    scopes: Option<&str>,
) -> Result<AuthSession, FmError> {
    let discovery = discover(issuer_url).await?;
    let (verifier, challenge) = generate_pkce();

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| FmError::Other(format!("Failed to bind callback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| FmError::Other(format!("Failed to get listener port: {e}")))?
        .port();

    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let scope = scopes.unwrap_or("openid");

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256",
        discovery.authorization_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(scope),
        urlencoding::encode(&challenge),
    );

    Ok(AuthSession {
        auth_url,
        listener,
        verifier,
        token_endpoint: discovery.token_endpoint,
        redirect_uri,
    })
}

pub async fn wait_for_callback(
    listener: TcpListener,
    timeout_secs: u64,
) -> Result<String, FmError> {
    let accept = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        listener.accept(),
    )
    .await
    .map_err(|_| FmError::Other("OIDC login timed out — no callback received".into()))?
    .map_err(|e| FmError::Other(format!("OIDC callback accept failed: {e}")))?;

    let (mut stream, _addr) = accept;

    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| FmError::Other(format!("OIDC callback read failed: {e}")))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse "GET /callback?code=XXXX&... HTTP/1.1"
    let code = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| {
            path.split('?')
                .nth(1)?
                .split('&')
                .find_map(|param| param.strip_prefix("code="))
        })
        .ok_or_else(|| FmError::Other("No authorization code in callback".into()))?
        .to_string();

    // Check for error parameter
    if let Some(err) = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| {
            path.split('?')
                .nth(1)?
                .split('&')
                .find_map(|param| param.strip_prefix("error="))
        })
    {
        let html = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h2>Authentication Failed</h2><p>{}</p>\
            <p>You can close this tab.</p></body></html>",
            urlencoding::decode(err).unwrap_or_default()
        );
        let _ = stream.write_all(html.as_bytes()).await;
        let _ = stream.shutdown().await;
        return Err(FmError::Other(format!(
            "OIDC authentication error: {}",
            urlencoding::decode(err).unwrap_or_default()
        )));
    }

    // Respond with success page
    let html = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>Authentication Successful</h2>\
        <p>You can close this tab and return to Furman.</p></body></html>";
    let _ = stream.write_all(html.as_bytes()).await;
    let _ = stream.shutdown().await;

    Ok(code)
}

// ── Token Exchange ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    access_token: Option<String>,
}

pub async fn exchange_code(
    token_endpoint: &str,
    code: &str,
    client_id: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OidcTokens, FmError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| FmError::Other(format!("Token exchange request failed: {e}")))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(FmError::Other(format!("Token exchange failed: {body}")));
    }

    let tokens: TokenResponse = resp
        .json()
        .await
        .map_err(|e| FmError::Other(format!("Token exchange parse failed: {e}")))?;

    let id_token = tokens
        .id_token
        .ok_or_else(|| FmError::Other("No id_token in token response".into()))?;

    Ok(OidcTokens {
        id_token,
        refresh_token: tokens.refresh_token,
    })
}

pub async fn refresh_tokens(
    token_endpoint: &str,
    refresh_token: &str,
    client_id: &str,
) -> Result<OidcTokens, FmError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(|e| FmError::Other(format!("Token refresh request failed: {e}")))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(FmError::Other(format!("Token refresh failed: {body}")));
    }

    let tokens: TokenResponse = resp
        .json()
        .await
        .map_err(|e| FmError::Other(format!("Token refresh parse failed: {e}")))?;

    let id_token = tokens
        .id_token
        .ok_or_else(|| FmError::Other("No id_token in refresh response".into()))?;

    Ok(OidcTokens {
        id_token,
        refresh_token: tokens.refresh_token,
    })
}
