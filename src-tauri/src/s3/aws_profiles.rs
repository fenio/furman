//! Local shared-profile discovery only. Never resolve credentials, execute
//! credential_process, read SSO token caches, or return file contents/keys.
#[cfg(test)]
#[path = "aws_profiles_tests.rs"]
mod tests;
use crate::models::FmError;
use aws_config::profile::ProfileSet;
use aws_runtime::env_config::file::{EnvConfigFileKind, EnvConfigFiles};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PROFILES: usize = 2_000;

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
}
fn error(message: &str) -> FmError {
    FmError::Other(message.into())
}

#[derive(Clone)]
pub struct ProfilePaths {
    pub config: PathBuf,
    pub credentials: PathBuf,
}

impl ProfilePaths {
    pub fn from_env() -> Result<Self, FmError> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .filter(|path| !path.as_os_str().is_empty());
        let path = |variable: &str, filename: &str| -> Result<PathBuf, FmError> {
            let path = if let Some(value) = std::env::var_os(variable) {
                PathBuf::from(value)
            } else {
                home.as_ref().ok_or_else(|| error("Home directory unavailable; configure AWS_CONFIG_FILE and AWS_SHARED_CREDENTIALS_FILE"))?.join(".aws").join(filename)
            };
            absolute_path(&path, home.as_deref(), &std::env::current_dir()?)
        };
        Ok(Self {
            config: path("AWS_CONFIG_FILE", "config")?,
            credentials: path("AWS_SHARED_CREDENTIALS_FILE", "credentials")?,
        })
    }
}

fn absolute_path(path: &Path, home: Option<&Path>, cwd: &Path) -> Result<PathBuf, FmError> {
    let path = if path
        .components()
        .next()
        .is_some_and(|part| part.as_os_str() == "~")
    {
        let mut components = path.components();
        components.next();
        home.ok_or_else(|| error("Cannot expand AWS shared-file home path"))?
            .join(components.as_path())
    } else {
        path.to_owned()
    };
    Ok(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

async fn read_file(path: &Path, label: &str) -> Result<String, FmError> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(FmError::Other(format!("Cannot inspect AWS {label} file ({:?}); check local file permissions. File contents were not included.", error.kind()))),
    };
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(error(
            "AWS shared files must be regular files no larger than 2 MiB each",
        ));
    }
    // O_NONBLOCK avoids waiting on a FIFO substituted between metadata/open.
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(path)
        .await
        .map_err(|_| error("Cannot open AWS shared file; check local permissions"))?;
    if !file
        .metadata()
        .await
        .map_err(|_| error("Cannot inspect opened AWS shared file"))?
        .is_file()
    {
        return Err(error("AWS shared files must be regular files"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| error("Cannot read AWS shared file; check local permissions"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(error("AWS shared file exceeds the 2 MiB discovery limit"));
    }
    String::from_utf8(bytes).map_err(|_| {
        error("AWS shared files must contain UTF-8 text; file contents were not included")
    })
}

pub async fn load(paths: &ProfilePaths) -> Result<ProfileSet, FmError> {
    let read = async {
        let config = read_file(&paths.config, "config").await?;
        let credentials = read_file(&paths.credentials, "credentials").await?;
        let files = EnvConfigFiles::builder()
            .with_contents(EnvConfigFileKind::Config, config)
            .with_contents(EnvConfigFileKind::Credentials, credentials)
            .build();
        aws_config::profile::load(&aws_types::os_shim_internal::Fs::real(), &aws_types::os_shim_internal::Env::real(), &files, None).await
            .map_err(|_| error("Cannot parse AWS shared profiles. Check config/credentials section syntax; file contents were not included."))
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), read)
        .await
        .map_err(|_| error("AWS profile discovery timed out while reading local files"))?
}

#[derive(Clone, Serialize)]
pub struct SharedProfile {
    pub name: String,
    pub region: Option<String>,
    pub sso: bool,
    pub login_profile: Option<String>,
    pub login_blocked_reason: Option<&'static str>,
    pub has_custom_endpoint: bool,
}

#[derive(Serialize)]
pub struct SharedProfiles {
    pub profiles: Vec<SharedProfile>,
    pub default_profile: Option<String>,
}

pub fn describe(profiles: &ProfileSet, name: &str) -> SharedProfile {
    let mut result = SharedProfile {
        name: name.into(),
        region: profiles
            .get_profile(name)
            .and_then(|profile| profile.get("region"))
            .filter(|region| {
                region.len() <= 128
                    && region
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
            .map(str::to_owned),
        sso: false,
        login_profile: None,
        login_blocked_reason: None,
        has_custom_endpoint: false,
    };
    let mut current = name;
    let mut seen = HashSet::new();
    let mut mixed = false;
    for _ in 0..32 {
        if !seen.insert(current) {
            result.login_blocked_reason =
                Some("Profile source chain contains a cycle; fix AWS shared configuration.");
            return result;
        }
        let Some(profile) = profiles.get_profile(current) else {
            result.login_blocked_reason =
                Some("Source profile is missing; fix AWS shared configuration.");
            return result;
        };
        result.has_custom_endpoint |=
            profile.get("endpoint_url").is_some() || profile.get("services").is_some();
        mixed |= [
            "aws_access_key_id",
            "aws_secret_access_key",
            "aws_session_token",
            "credential_process",
            "credential_source",
        ]
        .iter()
        .any(|name| profile.get(name).is_some());
        if profile.get("sso_session").is_some()
            || profile.get("sso_start_url").is_some()
            || profile.get("sso_account_id").is_some()
            || profile.get("sso_role_name").is_some()
        {
            result.sso = true;
            let settings_complete = if let Some(session) = profile.get("sso_session") {
                profiles.sso_session(session).is_some_and(|session| {
                    session
                        .get("sso_start_url")
                        .is_some_and(|value| !value.is_empty())
                        && session
                            .get("sso_region")
                            .is_some_and(|value| !value.is_empty())
                })
            } else {
                profile
                    .get("sso_start_url")
                    .is_some_and(|value| !value.is_empty())
                    && profile
                        .get("sso_region")
                        .is_some_and(|value| !value.is_empty())
            };
            result.login_blocked_reason = if mixed {
                Some("Mixed credential sources: use AWS CLI externally after reviewing this profile; Furman will not choose a sign-in identity.")
            } else if result.has_custom_endpoint {
                Some("Custom endpoint/services configuration: sign in using AWS CLI externally after reviewing which endpoints it will use.")
            } else if !settings_complete
                || !profile
                    .get("sso_account_id")
                    .is_some_and(|value| !value.is_empty())
                || !profile
                    .get("sso_role_name")
                    .is_some_and(|value| !value.is_empty())
            {
                Some("SSO profile/session configuration is incomplete. Configure it with AWS CLI v2 before signing in.")
            } else if !valid_name(current) {
                Some("SSO login profile name is invalid or exceeds 256 bytes.")
            } else {
                None
            };
            if result.login_blocked_reason.is_none() {
                result.login_profile = Some(current.into());
            }
            return result;
        }
        if let Some(source) = profile
            .get("source_profile")
            .filter(|_| profile.get("role_arn").is_some())
        {
            current = source;
        } else {
            return result;
        }
    }
    result.login_blocked_reason =
        Some("Profile source chain exceeds the 32-profile discovery limit.");
    result
}

pub fn summarize(profiles: &ProfileSet) -> Result<SharedProfiles, FmError> {
    let mut names: Vec<_> = profiles.profiles().collect();
    if names.len() > MAX_PROFILES {
        return Err(error("AWS discovery supports at most 2,000 shared profiles; no partial profile list was returned"));
    }
    names.sort_unstable();
    let default_profile =
        valid_name(profiles.selected_profile()).then(|| profiles.selected_profile().into());
    let profiles = names
        .into_iter()
        .filter(|name| valid_name(name))
        .map(|name| describe(profiles, name))
        .collect();
    Ok(SharedProfiles {
        profiles,
        default_profile,
    })
}

pub async fn discover() -> Result<SharedProfiles, FmError> {
    let profiles = load(&ProfilePaths::from_env()?).await?;
    summarize(&profiles)
}

pub const SSO_EXPIRED: &str = "[AWS_SSO_SESSION_EXPIRED] AWS IAM Identity Center session expired and could not be refreshed. Explicitly sign in with AWS CLI v2 for the intended SSO profile, then reconnect. Signing in does not change bucket permissions.";
pub const SSO_UNAVAILABLE: &str = "[AWS_SSO_SESSION_UNAVAILABLE] AWS IAM Identity Center cached session is unavailable or invalid. Review the intended SSO profile, sign in explicitly, or check local AWS cache permissions, then reconnect.";
pub const SSO_CONFIG: &str = "[AWS_SSO_CONFIGURATION] AWS IAM Identity Center profile/session configuration is incomplete or invalid. Review AWS shared configuration with AWS CLI v2; no automatic sign-in was attempted.";

/// Only classify credential-provider failures with SSO-specific evidence. Never
/// expose arbitrary source-chain messages, token-cache JSON or credential data.
pub fn sso_credential_hint(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    let mut source = Some(error);
    let mut credentials = false;
    let mut expired = false;
    let mut unavailable = false;
    let mut configuration = false;
    for _ in 0..20 {
        let Some(current) = source else {
            break;
        };
        credentials |= current.is::<aws_credential_types::provider::error::CredentialsError>();
        unavailable |= current
            .downcast_ref::<aws_sdk_sso::operation::get_role_credentials::GetRoleCredentialsError>()
            .is_some_and(|error| error.is_unauthorized_exception());
        let message = current
            .to_string()
            .chars()
            .take(2048)
            .collect::<String>()
            .to_ascii_lowercase();
        expired |= message.contains("sso token has expired")
            || message.contains("sso session has expired");
        unavailable |= message.contains("cached sso token")
            || message.contains("/sso/cache/")
            || message.contains("\\sso\\cache\\");
        configuration |=
            message.contains("invalid sso config") || message.contains("sso-session named");
        source = current.source();
    }
    if !credentials {
        None
    } else if expired {
        Some(SSO_EXPIRED)
    } else if configuration {
        Some(SSO_CONFIG)
    } else if unavailable {
        Some(SSO_UNAVAILABLE)
    } else {
        None
    }
}
