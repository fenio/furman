use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use russh::client;
use russh_keys::ssh_key;
use russh_sftp::client::SftpSession;

use crate::models::FmError;
use super::helpers::sftperr;

// ── State ────────────────────────────────────────────────────────────────────

pub struct SftpState(pub Mutex<HashMap<String, SftpConnection>>);

impl Default for SftpState {
    fn default() -> Self {
        SftpState(Mutex::new(HashMap::new()))
    }
}

pub struct SftpConnection {
    pub session: Arc<SftpSession>,
    pub ssh_handle: client::Handle<SshHandler>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub home_dir: String,
}

// ── SSH Handler ──────────────────────────────────────────────────────────────

/// Minimal SSH client handler — accepts all host keys (TODO: known_hosts).
pub struct SshHandler;

#[async_trait::async_trait]
impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all host keys for now
        // TODO: implement known_hosts verification
        Ok(true)
    }
}

// ── Client Builder ──────────────────────────────────────────────────────────

/// Establish an SSH connection and start the SFTP subsystem.
pub async fn build_sftp_client(
    host: &str,
    port: u16,
    username: &str,
    auth_method: &str,
    password: Option<&str>,
    key_path: Option<&str>,
    key_passphrase: Option<&str>,
) -> Result<SftpConnection, FmError> {
    let config = client::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(300)),
        keepalive_interval: Some(std::time::Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    };

    let handler = SshHandler;
    let mut handle = client::connect(Arc::new(config), (host, port), handler)
        .await
        .map_err(|e| sftperr(format!("SSH connect failed: {e}")))?;

    // Authenticate
    let authenticated = match auth_method {
        "password" => {
            let pw = password.ok_or_else(|| sftperr("Password required"))?;
            handle
                .authenticate_password(username, pw)
                .await
                .map_err(|e| sftperr(format!("Password auth failed: {e}")))?
        }
        "key" => {
            let path = key_path.ok_or_else(|| sftperr("Key path required"))?;
            let key = if let Some(pp) = key_passphrase {
                russh_keys::load_secret_key(path, Some(pp))
                    .map_err(|e| sftperr(format!("Failed to load key: {e}")))?
            } else {
                russh_keys::load_secret_key(path, None)
                    .map_err(|e| sftperr(format!("Failed to load key: {e}")))?
            };

            let key_with_alg = russh_keys::key::PrivateKeyWithHashAlg::new(
                Arc::new(key),
                Some(russh_keys::HashAlg::Sha256),
            )
            .map_err(|e| sftperr(format!("Key hash alg failed: {e}")))?;

            handle
                .authenticate_publickey(username, key_with_alg)
                .await
                .map_err(|e| sftperr(format!("Key auth failed: {e}")))?
        }
        "agent" => {
            let mut agent = russh_keys::agent::client::AgentClient::connect_env()
                .await
                .map_err(|e| sftperr(format!("SSH agent connect failed: {e}")))?;
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| sftperr(format!("SSH agent identities failed: {e}")))?;
            if identities.is_empty() {
                return Err(sftperr("No keys found in SSH agent"));
            }
            let mut authed = false;
            for identity in identities {
                match handle
                    .authenticate_publickey_with(username, identity.clone(), &mut agent)
                    .await
                {
                    Ok(true) => {
                        authed = true;
                        break;
                    }
                    _ => continue,
                }
            }
            authed
        }
        _ => return Err(sftperr(format!("Unknown auth method: {auth_method}"))),
    };

    if !authenticated {
        return Err(sftperr("Authentication failed"));
    }

    // Open a session channel and request the SFTP subsystem
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| sftperr(format!("Channel open failed: {e}")))?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| sftperr(format!("SFTP subsystem request failed: {e}")))?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| sftperr(format!("SFTP session init failed: {e}")))?;

    // Get the home directory
    let home_dir = sftp
        .canonicalize(".")
        .await
        .map_err(|e| sftperr(format!("Failed to resolve home dir: {e}")))?;

    Ok(SftpConnection {
        session: Arc::new(sftp),
        ssh_handle: handle,
        host: host.to_string(),
        port,
        username: username.to_string(),
        home_dir,
    })
}
