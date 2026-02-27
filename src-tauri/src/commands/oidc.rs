use crate::models::FmError;
use crate::oidc;
use serde::Serialize;

#[derive(Serialize)]
pub struct OidcAuthResult {
    pub id_token: String,
    pub refresh_token: Option<String>,
}

#[tauri::command]
pub async fn oidc_start_auth(
    issuer_url: String,
    client_id: String,
    scopes: Option<String>,
) -> Result<OidcAuthResult, FmError> {
    let session = oidc::start_auth(&issuer_url, &client_id, scopes.as_deref()).await?;

    // Open the browser
    std::process::Command::new("open")
        .arg(&session.auth_url)
        .spawn()
        .map_err(|e| FmError::Other(format!("Failed to open browser: {e}")))?;

    // Wait for the callback (120s timeout)
    let code = oidc::wait_for_callback(session.listener, 120).await?;

    // Exchange code for tokens
    let tokens = oidc::exchange_code(
        &session.token_endpoint,
        &code,
        &client_id,
        &session.verifier,
        &session.redirect_uri,
    )
    .await?;

    Ok(OidcAuthResult {
        id_token: tokens.id_token,
        refresh_token: tokens.refresh_token,
    })
}

#[tauri::command]
pub async fn oidc_refresh(
    issuer_url: String,
    client_id: String,
    refresh_token: String,
) -> Result<OidcAuthResult, FmError> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let resp = reqwest::get(&discovery_url)
        .await
        .map_err(|e| FmError::Other(format!("OIDC discovery failed: {e}")))?;

    #[derive(serde::Deserialize)]
    struct Disc {
        token_endpoint: String,
    }
    let disc: Disc = resp
        .json()
        .await
        .map_err(|e| FmError::Other(format!("OIDC discovery parse failed: {e}")))?;

    let tokens = oidc::refresh_tokens(&disc.token_endpoint, &refresh_token, &client_id).await?;

    Ok(OidcAuthResult {
        id_token: tokens.id_token,
        refresh_token: tokens.refresh_token,
    })
}
