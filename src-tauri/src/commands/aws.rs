//! Explicit AWS CLI v2 sign-in. No shell, no token/URL/code output in IPC/logs,
//! no automatic reconnect, credential resolution, or S3 operations.
#[cfg(test)]
#[path = "aws_tests.rs"]
mod tests;
use crate::{
    models::FmError,
    s3::aws_profiles::{self, ProfilePaths, SharedProfiles},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::State;
use tokio::{io::AsyncReadExt, process::Command};

fn error(message: &str) -> FmError {
    FmError::Other(message.into())
}

struct Job {
    id: String,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
pub struct AwsSsoState(Mutex<Option<Job>>);
struct JobGuard<'a> {
    state: &'a AwsSsoState,
    id: String,
    cancel: Arc<AtomicBool>,
}
impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.0.lock() {
            if active.as_ref().is_some_and(|job| job.id == self.id) {
                *active = None;
            }
        }
    }
}
impl AwsSsoState {
    fn begin(&self, id: &str) -> Result<JobGuard<'_>, FmError> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(error("Invalid AWS SSO operation ID"));
        }
        let mut active = self
            .0
            .lock()
            .map_err(|_| error("AWS SSO action state unavailable"))?;
        if active.is_some() {
            return Err(error(
                "An AWS SSO sign-in is already running; finish or cancel it first",
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Job {
            id: id.into(),
            cancel: cancel.clone(),
        });
        Ok(JobGuard {
            state: self,
            id: id.into(),
            cancel,
        })
    }
    fn cancel(&self, id: &str) -> Result<bool, FmError> {
        let active = self
            .0
            .lock()
            .map_err(|_| error("AWS SSO action state unavailable"))?;
        if let Some(job) = active.as_ref().filter(|job| job.id == id) {
            job.cancel.store(true, Ordering::Relaxed);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // GUI launches on macOS often omit Homebrew/AWS CLI install directories.
    #[cfg(target_os = "macos")]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/aws"),
        PathBuf::from("/usr/local/bin/aws"),
    ]);
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path)
                .filter(|path| path.is_absolute())
                .map(|path| path.join("aws")),
        );
    }
    candidates.extend([
        PathBuf::from("/usr/local/bin/aws"),
        PathBuf::from("/usr/bin/aws"),
    ]);
    candidates
}

fn find_cli() -> Result<PathBuf, FmError> {
    cli_candidates().into_iter().find(|path| path.is_file()).ok_or_else(|| error("AWS CLI v2 was not found. Install AWS CLI v2 and ensure aws is on an absolute PATH entry; macOS Homebrew and /usr/local/bin are also checked. No sign-in was started."))
}

async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn bounded_output(reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, FmError> {
    let mut bytes = Vec::new();
    reader
        .take(1025)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| error("Cannot read AWS CLI version response"))?;
    if bytes.len() > 1024 {
        return Err(error(
            "AWS CLI version response exceeded its safety limit; no sign-in was started",
        ));
    }
    Ok(bytes)
}

fn parse_version(stdout: &[u8], stderr: &[u8]) -> Result<String, FmError> {
    for bytes in [stdout, stderr] {
        if let Ok(text) = std::str::from_utf8(bytes) {
            if let Some(version) = text
                .trim()
                .strip_prefix("aws-cli/")
                .and_then(|text| text.split_whitespace().next())
            {
                let parts: Vec<_> = version.split('.').collect();
                if version.len() <= 32
                    && parts.len() == 3
                    && parts[0] == "2"
                    && parts.iter().all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                    })
                {
                    return Ok(version.into());
                }
            }
        }
    }
    Err(error("AWS CLI v2 is required for this sign-in action. Install/upgrade AWS CLI v2; raw command output was not included."))
}

fn command(program: &Path, paths: &ProfilePaths) -> Command {
    let mut command = Command::new(program);
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .env("AWS_CONFIG_FILE", &paths.config)
        .env("AWS_SHARED_CREDENTIALS_FILE", &paths.credentials)
        .env("AWS_PAGER", "")
        .env("AWS_CLI_AUTO_PROMPT", "off");
    command
}

async fn version(
    program: &Path,
    paths: &ProfilePaths,
    cancel: &AtomicBool,
) -> Result<String, FmError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(error("AWS SSO sign-in cancelled before launch"));
    }
    let mut child = command(program, paths).arg("--version").stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|failure| error(&format!("Could not start AWS CLI version check ({:?}). Verify the CLI installation and executable permissions; no sign-in was started.", failure.kind())))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("AWS CLI version stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| error("AWS CLI version stderr unavailable"))?;
    let result = tokio::select! {
        result = async {
            let (status, stdout, stderr) = tokio::try_join!(async { child.wait().await.map_err(|_| error("AWS CLI version check failed")) }, bounded_output(stdout), bounded_output(stderr))?;
            if !status.success() { return Err(error("AWS CLI version check returned an error. Verify AWS CLI v2 installation; raw output was not included.")); }
            parse_version(&stdout, &stderr)
        } => result,
        _ = tokio::time::sleep(Duration::from_secs(5)) => Err(error("AWS CLI version check timed out; no sign-in was started")),
        _ = cancelled(cancel) => Err(error("AWS SSO sign-in cancelled during CLI version check")),
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

#[derive(Serialize)]
pub struct SsoLoginResult {
    pub login_profile: String,
    pub cli_version: String,
}

async fn login(
    program: &Path,
    paths: &ProfilePaths,
    login_profile: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<SsoLoginResult, FmError> {
    let cli_version = version(program, paths, cancel).await?;
    if cancel.load(Ordering::Relaxed) {
        return Err(error("AWS SSO sign-in cancelled before browser launch"));
    }
    // One argument, including '=': a profile named '--debug' or containing shell
    // metacharacters cannot inject CLI options or a shell command.
    let mut child = command(program, paths)
        .args(["sso", "login"])
        .arg(format!("--profile={login_profile}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|failure| {
            error(&format!(
                "Could not start AWS CLI SSO login ({:?}). Verify the CLI installation and permissions.",
                failure.kind()
            ))
        })?;
    let result = tokio::select! {
        status = child.wait() => match status {
            Ok(status) if status.success() => Ok(SsoLoginResult { login_profile: login_profile.into(), cli_version }),
            Ok(status) => Err(FmError::Other(format!("AWS CLI SSO login did not complete (exit {}). Check AWS profile/session configuration, browser availability, CLI proxy/network settings and AWS cache permissions. Run AWS CLI v2 externally for its diagnostics; browser URLs, authorization codes and raw output are deliberately not copied into Furman.", status.code().map(|code| code.to_string()).unwrap_or_else(|| "terminated".into())))),
            Err(_) => Err(error("Could not wait for AWS CLI SSO login. Check the AWS CLI installation.")),
        },
        _ = tokio::time::sleep(timeout) => Err(error("AWS CLI SSO login exceeded 5 minutes and was stopped. Browser tabs or AWS-managed cached state may remain; no automatic retry or reconnect was performed.")),
        _ = cancelled(cancel) => Err(error("AWS CLI SSO login cancelled. Browser tabs or AWS-managed cached state may remain; no automatic retry or reconnect was performed.")),
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

#[tauri::command]
pub async fn aws_shared_profiles() -> Result<SharedProfiles, FmError> {
    aws_profiles::discover().await
}

fn resolve_login(report: &SharedProfiles, profile: &str, expected: &str) -> Result<(), FmError> {
    let selected = report
        .profiles
        .iter()
        .find(|entry| entry.name == profile)
        .ok_or_else(|| {
            error("Selected AWS shared profile no longer exists; refresh profile discovery")
        })?;
    let actual = selected.login_profile.as_deref().ok_or_else(|| {
        error(selected.login_blocked_reason.unwrap_or(
            "Selected profile does not declare an eligible AWS SSO session; no command was run",
        ))
    })?;
    if actual != expected {
        return Err(error("SSO source profile changed since discovery; refresh and approve the intended profile again"));
    }
    Ok(())
}

#[tauri::command]
pub async fn aws_sso_login(
    state: State<'_, AwsSsoState>,
    op_id: String,
    profile: String,
    login_profile: String,
    approved: bool,
) -> Result<SsoLoginResult, FmError> {
    if !approved {
        return Err(error(
            "Explicit approval is required before running AWS CLI SSO login",
        ));
    }
    if !aws_profiles::valid_name(&profile) {
        return Err(error(
            "Select a valid existing AWS shared profile before signing in",
        ));
    }
    if [
        "AWS_ENDPOINT_URL",
        "AWS_ENDPOINT_URL_SSO",
        "AWS_ENDPOINT_URL_SSO_OIDC",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Err(error("An AWS/SSO endpoint override is set in the CLI environment. Review it and sign in externally; Furman will not change endpoint settings or run a command in this configuration."));
    }
    let job = state.begin(&op_id)?;
    let paths = ProfilePaths::from_env()?;
    {
        let profiles = aws_profiles::load(&paths).await?;
        let report = aws_profiles::summarize(&profiles)?;
        resolve_login(&report, &profile, &login_profile)?;
    } // Do not keep parsed credential-file contents alive during browser sign-in.
    login(
        &find_cli()?,
        &paths,
        &login_profile,
        &job.cancel,
        Duration::from_secs(300),
    )
    .await
}

#[tauri::command]
pub fn aws_sso_cancel(state: State<'_, AwsSsoState>, op_id: String) -> Result<bool, FmError> {
    state.cancel(&op_id)
}
