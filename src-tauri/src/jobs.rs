//! Job definitions and configured-identity tracking. Runtime read-only binding
//! lives in job_runs; configuration pins are NOT authorization to execute a job.
use crate::models::FmError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
fn error(message: &str) -> FmError {
    FmError::Other(message.into())
}
fn io_error(failure: std::io::Error) -> FmError {
    error(&format!("Saved-job identity/configuration storage is unavailable ({:?}). Check local permissions and disk space. A save/edit may already have completed if finalization failed; refresh and review before retrying. No file contents or credential values were included.", failure.kind()))
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

#[derive(Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub store: PathBuf,
}
impl Paths {
    pub fn home() -> Result<Self, FmError> {
        let directory = dirs::home_dir()
            .ok_or_else(|| error("Home directory unavailable for saved jobs"))?
            .join(".config/furman");
        Ok(Self {
            config: directory.join("config.json"),
            store: directory.join("saved-jobs.json"),
        })
    }
}

fn read_bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, FmError> {
    match fs::symlink_metadata(path) {
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(failure) => return Err(io_error(failure)),
        Ok(metadata) if !metadata.is_file() || metadata.len() > limit => return Err(error("Saved-job/configuration storage must be a regular, bounded file; symlinks and oversized files are not supported")),
        Ok(_) => {}
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(error("Saved-job storage must be a regular file"));
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > limit {
        return Err(error(
            "Saved-job/configuration storage exceeds its size limit",
        ));
    }
    Ok(Some(bytes))
}

struct IdentityLock(File);
impl Drop for IdentityLock {
    fn drop(&mut self) {
        // Release explicitly: a concurrently spawned child may briefly retain
        // a fork-inherited descriptor until exec, even with close-on-exec set.
        let _ = self.0.unlock();
    }
}
fn lock(paths: &Paths) -> Result<IdentityLock, FmError> {
    fs::create_dir_all(
        paths
            .store
            .parent()
            .ok_or_else(|| error("Saved-job directory unavailable"))?,
    )
    .map_err(io_error)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(paths.store.with_extension("lock"))
        .map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(error("Saved-job lock must be a regular file"));
    }
    file.try_lock().map_err(|_| error("Saved-job identity storage is busy in another operation. Retry after it finishes; no edit was started."))?;
    Ok(IdentityLock(file))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    value: String,
    ready: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    generations: BTreeMap<String, Generation>,
    jobs: Vec<Job>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfiguration {
    pub bucket: String,
    pub region: String,
    /// Declared endpoint intent only. None is NOT proof of effective AWS routing.
    pub endpoint: Option<String>,
    pub provider: String,
    pub proxy_endpoint: Option<String>,
    pub proxy_auth: bool,
    pub sse_customer_key_enabled: bool,
    pub acceleration: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Pin {
    pub profile_id: String,
    pub generation: String,
    pub configuration: ProfileConfiguration,
    pub prefix: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourcePin {
    Local {
        path: String,
        device: String,
        inode: String,
    },
    S3 {
        pin: S3Pin,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub revision: String,
    pub name: String,
    pub source: SourcePin,
    pub destination: S3Pin,
    pub exclude_patterns: Vec<String>,
    pub compare_mode: JobMode,
    pub operation: JobOperation,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobMode {
    Sha256,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOperation {
    CopyUpdateOnly,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRequest {
    Local { path: String },
    S3 { profile_id: String, prefix: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    pub expected_revision: String,
    pub expected_configuration_revision: String,
    pub id: Option<String>,
    pub name: String,
    pub source: SourceRequest,
    pub destination_profile_id: String,
    pub destination_prefix: String,
    pub exclude_patterns: Vec<String>,
    pub acknowledged: bool,
}

#[derive(Serialize)]
pub struct ProfileOption {
    pub id: String,
    pub name: String,
    pub configuration: Option<ProfileConfiguration>,
    pub unavailable: Option<String>,
}
#[derive(Serialize)]
pub struct JobView {
    pub definition: Job,
    pub status: &'static str,
    pub reason: &'static str,
}
#[derive(Serialize)]
pub struct Report {
    pub revision: String,
    pub configuration_revision: String,
    pub jobs: Vec<JobView>,
    pub profiles: Vec<ProfileOption>,
    pub execution_available: bool,
}

fn safe_url(value: &str) -> Result<String, FmError> {
    let tail = value
        .split_once("://")
        .map(|(_, tail)| tail)
        .ok_or_else(|| error("Job endpoints must be absolute HTTP(S) URLs"))?;
    let boundary = tail.find(['/', '?', '#']).unwrap_or(tail.len());
    if value.trim() != value
        || value.contains('\\')
        || tail[..boundary].contains('@')
        || !matches!(&tail[boundary..], "" | "/")
    {
        return Err(error("Job endpoint/proxy intent must be a root URL without userinfo, paths, query strings or fragments"));
    }
    let url =
        url::Url::parse(value).map_err(|_| error("Job endpoints must be absolute HTTP(S) URLs"))?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || !valid_text(value, 2048)
    {
        return Err(error("Job endpoints/proxies cannot contain credentials, query strings, fragments or non-root paths. Ordinary connections remain available."));
    }
    Ok(url.to_string())
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn configuration(value: &Value) -> Result<ProfileConfiguration, FmError> {
    if [
        "bucket",
        "region",
        "endpoint",
        "provider",
        "proxyUrl",
        "proxyUsername",
        "roleArn",
        "profile",
        "externalId",
        "oidcIssuerUrl",
        "oidcClientId",
    ]
    .iter()
    .any(|key| {
        value
            .get(key)
            .is_some_and(|value| !value.is_null() && !value.is_string())
    }) || [
        "sseCustomerKeyEnabled",
        "defaultClientEncryption",
        "useTransferAcceleration",
    ]
    .iter()
    .any(|key| {
        value
            .get(key)
            .is_some_and(|value| !value.is_null() && !value.is_boolean())
    }) {
        return Err(error(
            "Saved job profile configuration has invalid field types",
        ));
    }
    if text(value, "type") != "s3"
        || text(value, "credentialType") != "keychain"
        || text(value, "accessKeyId").is_empty()
    {
        return Err(error("Definitions initially require a saved manual-key S3 profile; no AWS credential chain or sign-in is resolved"));
    }
    if [
        "roleArn",
        "profile",
        "externalId",
        "oidcIssuerUrl",
        "oidcClientId",
    ]
    .iter()
    .any(|key| !text(value, key).is_empty())
        || flag(value, "defaultClientEncryption")
    {
        return Err(error("Mixed/assumed-role/OIDC credentials and client-side password encryption are not supported by job definitions"));
    }
    let bucket = text(value, "bucket");
    let region = text(value, "region");
    if bucket.is_empty()
        || !valid_text(bucket, 255)
        || bucket
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch.is_whitespace())
        || region.is_empty()
        || !valid_text(region, 128)
        || !region
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(error("Job profile has an invalid bucket or region"));
    }
    let endpoint = (!text(value, "endpoint").is_empty())
        .then(|| safe_url(text(value, "endpoint")))
        .transpose()?;
    let proxy = text(value, "proxyUrl");
    if proxy == "system" {
        return Err(error("System-proxy routing is not pinned by this foundation. Use ordinary connections or an explicit proxy configuration."));
    }
    let provider = text(value, "provider");
    let provider = if provider.is_empty() {
        if endpoint.is_some() {
            "custom"
        } else {
            "aws"
        }
    } else {
        provider
    };
    if !valid_id(provider) {
        return Err(error("Job provider identifier is invalid"));
    }
    if endpoint.is_none() && provider != "aws" {
        return Err(error("A non-AWS job profile requires an explicit root endpoint. A missing endpoint is not treated as proof of AWS routing."));
    }
    if flag(value, "sseCustomerKeyEnabled")
        && endpoint
            .as_deref()
            .is_some_and(|endpoint| !endpoint.starts_with("https://"))
    {
        return Err(error("SSE-C job definitions require HTTPS"));
    }
    Ok(ProfileConfiguration {
        bucket: bucket.into(),
        region: region.into(),
        endpoint,
        provider: provider.into(),
        proxy_endpoint: (!proxy.is_empty()).then(|| safe_url(proxy)).transpose()?,
        proxy_auth: !text(value, "proxyUsername").is_empty(),
        sse_customer_key_enabled: flag(value, "sseCustomerKeyEnabled"),
        acceleration: flag(value, "useTransferAcceleration"),
    })
}

fn profiles(paths: &Paths) -> Result<BTreeMap<String, Value>, FmError> {
    let Some(bytes) = read_bytes(&paths.config, MAX_CONFIG_BYTES)? else {
        return Ok(BTreeMap::new());
    };
    parse_profiles(&bytes)
}
fn parse_profiles(bytes: &[u8]) -> Result<BTreeMap<String, Value>, FmError> {
    let config: Value = serde_json::from_slice(bytes).map_err(|_| {
        error("Cannot parse saved connection configuration; no contents were included")
    })?;
    let Some(list) = config
        .get("connections")
        .or_else(|| config.get("s3Profiles"))
        .and_then(Value::as_array)
    else {
        if config.is_object()
            && config.get("connections").is_none()
            && config.get("s3Profiles").is_none()
        {
            return Ok(BTreeMap::new());
        }
        return Err(error(
            "Saved connections must be an array; job definitions were not modified",
        ));
    };
    if list.len() > 2000 {
        return Err(error(
            "At most 2,000 saved connections can be inspected for job definitions",
        ));
    }
    let mut result = BTreeMap::new();
    for profile in list {
        let id = text(profile, "id");
        if !valid_id(id) {
            continue;
        }
        let mut profile = profile.clone();
        if profile.get("type").is_none() {
            profile["type"] = "s3".into();
        }
        if result.insert(id.into(), profile).is_some() {
            return Err(error(
                "Saved connection IDs are ambiguous; repair duplicate IDs before saving jobs",
            ));
        }
    }
    Ok(result)
}

fn validate_options(name: &str, exclusions: &[String]) -> Result<(), FmError> {
    if name.is_empty() || name != name.trim() || name.chars().count() > 80 || !valid_text(name, 320)
    {
        return Err(error(
            "Use a unique job name of 1–80 characters without surrounding whitespace or controls",
        ));
    }
    if exclusions.len() > 128
        || exclusions.iter().any(|value| {
            value.is_empty()
                || value.trim() != value
                || !valid_text(value, 512)
                || value.contains(',')
        })
    {
        return Err(error("Use at most 128 explicit exclusions of 1–512 bytes without commas, surrounding whitespace or controls"));
    }
    Ok(())
}
fn validate_pin(pin: &S3Pin) -> Result<(), FmError> {
    if !valid_id(&pin.profile_id)
        || uuid::Uuid::parse_str(&pin.generation).is_err()
        || !valid_prefix(&pin.prefix)
    {
        return Err(error("Saved job contains an invalid S3 configuration pin"));
    }
    // Validate deserialized endpoint projections too, not only newly captured pins.
    if let Some(endpoint) = &pin.configuration.endpoint {
        if safe_url(endpoint)? != *endpoint {
            return Err(error("Saved endpoint pin is not canonical"));
        }
    }
    if let Some(proxy) = &pin.configuration.proxy_endpoint {
        if safe_url(proxy)? != *proxy {
            return Err(error("Saved proxy pin is not canonical"));
        }
    }
    let config = &pin.configuration;
    if config.bucket.is_empty()
        || !valid_text(&config.bucket, 255)
        || config
            .bucket
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch.is_whitespace())
        || config.region.is_empty()
        || !valid_text(&config.region, 128)
        || !config
            .region
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !valid_id(&config.provider)
        || (config.endpoint.is_none() && config.provider != "aws")
        || (config.sse_customer_key_enabled
            && config
                .endpoint
                .as_deref()
                .is_some_and(|endpoint| !endpoint.starts_with("https://")))
    {
        return Err(error("Saved S3 configuration pin is invalid"));
    }
    Ok(())
}
fn validate_store(store: &Store) -> Result<(), FmError> {
    if store.version != 1 || store.jobs.len() > 100 || store.generations.len() > 4096 {
        return Err(error(
            "Unsupported or oversized saved-job document; it was not reset",
        ));
    }
    if store
        .generations
        .iter()
        .any(|(id, value)| !valid_id(id) || uuid::Uuid::parse_str(&value.value).is_err())
    {
        return Err(error("Saved-job identity generations are invalid"));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for job in &store.jobs {
        validate_options(&job.name, &job.exclude_patterns)?;
        validate_pin(&job.destination)?;
        if uuid::Uuid::parse_str(&job.id).is_err()
            || !job
                .revision
                .parse::<u64>()
                .is_ok_and(|revision| revision > 0)
            || !ids.insert(&job.id)
            || !names.insert(job.name.to_lowercase())
        {
            return Err(error(
                "Saved job IDs, revisions or names are invalid/duplicated",
            ));
        }
        match &job.source {
            SourcePin::S3 { pin } => {
                validate_pin(pin)?;
                reject_overlap(pin, &job.destination)?;
            }
            SourcePin::Local {
                path,
                device,
                inode,
            } => {
                if !Path::new(path).is_absolute()
                    || !valid_text(path, 4096)
                    || device.parse::<u64>().is_err()
                    || inode.parse::<u64>().is_err()
                {
                    return Err(error("Saved local source pin is invalid"));
                }
            }
        }
    }
    Ok(())
}
fn load(paths: &Paths) -> Result<(String, Store), FmError> {
    let Some(bytes) = read_bytes(&paths.store, MAX_BYTES)? else {
        return Ok((
            "missing".into(),
            Store {
                version: 1,
                ..Store::default()
            },
        ));
    };
    let store: Store = serde_json::from_slice(&bytes).map_err(|_| error("Saved-job storage is malformed or contains unsupported fields; repair/back it up externally. No contents were included and it was not reset."))?;
    validate_store(&store)?;
    Ok((digest(&bytes), store))
}
fn commit(paths: &Paths, store: &Store) -> Result<String, FmError> {
    validate_store(store)?;
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|_| error("Could not encode saved-job storage"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(error(
            "Saved-job definitions/identity markers exceed the 1 MiB limit",
        ));
    }
    replace(&paths.store, &bytes)?;
    Ok(digest(&bytes))
}
fn replace(path: &Path, bytes: &[u8]) -> Result<(), FmError> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Saved-job/configuration directory unavailable"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    temporary.write_all(bytes).map_err(io_error)?;
    temporary.as_file().sync_all().map_err(io_error)?;
    temporary
        .persist(path)
        .map_err(|failure| io_error(failure.error))?;
    // The pending identity marker must survive a rename before an OS-keychain
    // mutation is allowed to start. This is not an execution-outcome journal.
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)?;
    Ok(())
}

fn capture_local(path: &str) -> Result<SourcePin, FmError> {
    Ok(open_local_root(path)?.1)
}
pub(crate) fn open_local_root(path: &str) -> Result<(File, SourcePin), FmError> {
    use nix::{
        fcntl::{open, openat, OFlag},
        sys::stat::Mode,
    };
    if !Path::new(path).is_absolute() || !valid_text(path, 4096) {
        return Err(error("Select an absolute UTF-8 local source directory"));
    }
    let flags = OFlag::O_RDONLY
        | OFlag::O_DIRECTORY
        | OFlag::O_CLOEXEC
        | OFlag::O_NOFOLLOW
        | OFlag::O_NONBLOCK;
    let mut directory = File::from(
        open(Path::new("/"), flags, Mode::empty())
            .map_err(|_| error("Cannot open local source root"))?,
    );
    let mut canonical = PathBuf::from("/");
    for component in Path::new(path).components().skip(1) {
        let Component::Normal(name) = component else {
            return Err(error(
                "Local job roots cannot contain parent-directory components",
            ));
        };
        directory = File::from(openat(&directory, name, flags, Mode::empty()).map_err(|_| {
            error("Local source is unavailable, contains symlinks, or is not a directory")
        })?);
        canonical.push(name);
    }
    let metadata = directory.metadata().map_err(io_error)?;
    let pin = SourcePin::Local {
        path: canonical
            .to_str()
            .ok_or_else(|| error("Local source must be UTF-8"))?
            .into(),
        device: metadata.dev().to_string(),
        inode: metadata.ino().to_string(),
    };
    Ok((directory, pin))
}
fn reject_overlap(source: &S3Pin, destination: &S3Pin) -> Result<(), FmError> {
    if source.configuration.endpoint == destination.configuration.endpoint
        && source.configuration.bucket == destination.configuration.bucket
    {
        let ancestor = |left: &str, right: &str| {
            left == right
                || left.is_empty()
                || (left.ends_with('/') && right.starts_with(left))
                || right
                    .strip_prefix(left)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        if ancestor(&source.prefix, &destination.prefix)
            || ancestor(&destination.prefix, &source.prefix)
        {
            return Err(error("Source and destination overlap in the same configured S3 namespace; choose disjoint prefixes"));
        }
    }
    Ok(())
}
fn current_pin(
    id: &str,
    prefix: &str,
    config: &BTreeMap<String, Value>,
    store: &mut Store,
) -> Result<S3Pin, FmError> {
    if !valid_id(id) || !valid_prefix(prefix) {
        return Err(error("Select a valid saved profile; a nonempty job folder prefix must end with '/'. Prefixes are never rewritten automatically."));
    }
    let configuration = configuration(config.get(id).ok_or_else(|| {
        error("Saved profile no longer exists; refresh and review the definition")
    })?)?;
    let marker = store
        .generations
        .entry(id.into())
        .or_insert_with(|| Generation {
            value: uuid(),
            ready: true,
        });
    if !marker.ready {
        return Err(error("A managed profile/keychain edit did not complete. Resave that connection/secret successfully, then explicitly rebind this definition."));
    }
    Ok(S3Pin {
        profile_id: id.into(),
        generation: marker.value.clone(),
        configuration,
        prefix: prefix.into(),
    })
}
fn valid_prefix(prefix: &str) -> bool {
    valid_text(prefix, 2048) && (prefix.is_empty() || prefix.ends_with('/'))
}
fn pin_matches(pin: &S3Pin, config: &BTreeMap<String, Value>, store: &Store) -> bool {
    store
        .generations
        .get(&pin.profile_id)
        .is_some_and(|marker| marker.ready && marker.value == pin.generation)
        && config
            .get(&pin.profile_id)
            .and_then(|profile| configuration(profile).ok())
            .is_some_and(|value| value == pin.configuration)
}
fn report(config: BTreeMap<String, Value>, revision: String, store: Store) -> Report {
    let configuration_revision = configuration_revision(&config);
    let profiles = config
        .iter()
        .filter(|(_, profile)| text(profile, "type") == "s3")
        .map(|(id, profile)| {
            let result = configuration(profile);
            let name = text(profile, "name");
            ProfileOption {
                id: id.clone(),
                name: if valid_text(name, 320) {
                    name.into()
                } else {
                    "Saved connection".into()
                },
                configuration: result.as_ref().ok().cloned(),
                unavailable: result.err().map(|failure| failure.to_string()),
            }
        })
        .collect();
    let jobs = store.jobs.iter().map(|job| {
        let valid_source = match &job.source { SourcePin::S3 { pin } => pin_matches(pin, &config, &store), SourcePin::Local { path, .. } => capture_local(path).is_ok_and(|value| value == job.source) };
        let valid = valid_source && pin_matches(&job.destination, &config, &store);
        JobView { definition: job.clone(), status: if valid { "definition_valid" } else { "needs_rebind" }, reason: if valid { "Configuration pins match. Explicit native read-only binding/comparison is available; job writes remain disabled." } else { "A profile, managed generation or local root changed/is unavailable. Review both roots and explicitly save a rebound definition." } }
    }).collect();
    Report {
        revision,
        configuration_revision,
        jobs,
        profiles,
        execution_available: false,
    }
}
fn configuration_revision(config: &BTreeMap<String, Value>) -> String {
    // Hash ONLY the allowlisted, non-secret projection. Access-key IDs,
    // usernames, keychain data and arbitrary config fields never enter a hash.
    let safe: BTreeMap<_, _> = config
        .iter()
        .map(|(id, profile)| (id, configuration(profile).ok()))
        .collect();
    digest(&serde_json::to_vec(&safe).expect("serializable configuration projection"))
}
pub fn list(paths: &Paths) -> Result<Report, FmError> {
    let (revision, store) = load(paths)?;
    Ok(report(profiles(paths)?, revision, store))
}

/// Re-resolve native persisted profiles under the same lock used by managed
/// configuration/keychain edits. The callback is synchronous; no network await
/// is allowed while holding this identity lock.
pub(crate) fn with_binding_snapshot<T>(
    paths: &Paths,
    id: &str,
    revision: &str,
    read: impl FnOnce(&Job, &BTreeMap<String, Value>) -> Result<T, FmError>,
) -> Result<T, FmError> {
    let _lock = lock(paths)?;
    let (_, store) = load(paths)?;
    let job = store
        .jobs
        .iter()
        .find(|job| job.id == id && job.revision == revision)
        .ok_or_else(|| {
            error(
                "Job was changed or deleted. Refresh the definition and explicitly bind a new run.",
            )
        })?;
    let config = profiles(paths)?;
    if !pin_matches(&job.destination, &config, &store)
        || matches!(&job.source, SourcePin::S3 { pin } if !pin_matches(pin, &config, &store))
    {
        return Err(error("Job profile configuration or managed secret generation changed. Explicitly rebind the definition before connecting or comparing."));
    }
    if let SourcePin::Local { path, .. } = &job.source {
        if capture_local(path)? != job.source {
            return Err(error(
                "Local job root was replaced; explicitly rebind the definition",
            ));
        }
    }
    read(job, &config)
}

pub(crate) fn validate_binding(paths: &Paths, expected: &Job) -> Result<(), FmError> {
    with_binding_snapshot(paths, &expected.id, &expected.revision, |job, _| {
        // Exact definition equality also catches unsupported external edits that
        // neglected to increment the definition revision.
        if serde_json::to_vec(job).ok() != serde_json::to_vec(expected).ok() {
            return Err(error(
                "Job definition changed; discard the run and bind again",
            ));
        }
        Ok(())
    })
}
pub fn save(paths: &Paths, request: SaveRequest) -> Result<Report, FmError> {
    if !request.acknowledged {
        return Err(error(
            "Explicitly acknowledge source → destination before saving/rebinding a definition",
        ));
    }
    validate_options(&request.name, &request.exclude_patterns)?;
    let _lock = lock(paths)?;
    let (revision, mut store) = load(paths)?;
    if revision != request.expected_revision {
        return Err(error("Job definitions or identity generations changed; refresh and review before saving. No definition was saved."));
    }
    let id = request.id.clone().unwrap_or_else(uuid);
    if request.id.is_some() && !store.jobs.iter().any(|job| job.id == id) {
        return Err(error(
            "Selected job no longer exists; refresh before rebinding",
        ));
    }
    if store
        .jobs
        .iter()
        .any(|job| job.id != id && job.name.to_lowercase() == request.name.to_lowercase())
    {
        return Err(error("Job names must be unique, ignoring case"));
    }
    let config = profiles(paths)?;
    if configuration_revision(&config) != request.expected_configuration_revision {
        return Err(error("Saved profile configuration changed since review. Refresh and explicitly acknowledge both roots again; no definition was saved."));
    }
    let destination = current_pin(
        &request.destination_profile_id,
        &request.destination_prefix,
        &config,
        &mut store,
    )?;
    let source = match request.source {
        SourceRequest::Local { path } => capture_local(&path)?,
        SourceRequest::S3 { profile_id, prefix } => {
            let pin = current_pin(&profile_id, &prefix, &config, &mut store)?;
            reject_overlap(&pin, &destination)?;
            SourcePin::S3 { pin }
        }
    };
    let next_revision = store
        .jobs
        .iter()
        .find(|job| job.id == id)
        .map(|job| job.revision.parse::<u64>().unwrap_or(u64::MAX))
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| error("Job revision limit reached; create a new definition instead"))?;
    let job = Job {
        id: id.clone(),
        revision: next_revision.to_string(),
        name: request.name,
        source,
        destination,
        exclude_patterns: request.exclude_patterns,
        compare_mode: JobMode::Sha256,
        operation: JobOperation::CopyUpdateOnly,
    };
    if let Some(existing) = store.jobs.iter_mut().find(|job| job.id == id) {
        *existing = job;
    } else {
        store.jobs.push(job);
    }
    let revision = commit(paths, &store)?;
    Ok(report(config, revision, store))
}
pub fn delete(
    paths: &Paths,
    expected: &str,
    id: &str,
    acknowledged: bool,
) -> Result<Report, FmError> {
    if !acknowledged {
        return Err(error("Confirm deletion of the saved definition"));
    }
    let _lock = lock(paths)?;
    let (revision, mut store) = load(paths)?;
    if revision != expected {
        return Err(error("Saved jobs changed; refresh before deleting"));
    }
    if !store.jobs.iter().any(|job| job.id == id) {
        return Err(error("Saved job no longer exists"));
    }
    let config = profiles(paths)?;
    store.jobs.retain(|job| job.id != id);
    let revision = commit(paths, &store)?;
    Ok(report(config, revision, store))
}

/// Durable random generations, NOT hashes of credentials. Serialize all managed
/// identity edits against definition saves across cooperating app processes.
fn identity_edit<T>(
    paths: &Paths,
    ids: Vec<String>,
    operation: impl FnOnce() -> Result<T, FmError>,
) -> Result<T, FmError> {
    let _lock = lock(paths)?;
    let (_, mut store) = load(paths)?;
    if ids.is_empty() {
        return operation();
    }
    for id in &ids {
        store.generations.insert(
            id.clone(),
            Generation {
                value: uuid(),
                ready: false,
            },
        );
    }
    commit(paths, &store)?; // Invalidate BEFORE the credential/configuration mutation.
    let result = operation()?; // A failed/ambiguous edit leaves ready=false on disk.
    for id in ids {
        if let Some(marker) = store.generations.get_mut(&id) {
            marker.ready = true;
        }
    }
    commit(paths, &store)?;
    Ok(result)
}
pub fn secret_edit<T>(
    paths: &Paths,
    entry_id: &str,
    operation: impl FnOnce() -> Result<T, FmError>,
) -> Result<T, FmError> {
    let base = [":sse-c", ":proxy", ":oidc-refresh"]
        .iter()
        .find_map(|suffix| entry_id.strip_suffix(suffix))
        .unwrap_or(entry_id);
    // Legacy/imported IDs outside the job schema remain usable ordinarily.
    // No eligible job can reference such an ID, so there is nothing to bind.
    if !valid_id(base) {
        return operation();
    }
    identity_edit(paths, vec![base.into()], operation)
}
pub fn write_config(paths: &Paths, content: &str) -> Result<(), FmError> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        return Err(error(
            "Saved configuration exceeds the 4 MiB identity-inspection limit",
        ));
    }
    let next = parse_profiles(content.as_bytes())?;
    // This lock is acquired before reading the old config so concurrent managed
    // edits cannot classify against the same stale pre-edit profile set.
    let _lock = lock(paths)?;
    let (_, mut store) = load(paths)?;
    let before = profiles(paths)?;
    let comparable = |value: &Value| {
        let mut value = value.clone();
        if let Some(map) = value.as_object_mut() {
            map.remove("name");
        }
        value
    };
    let ids: HashSet<_> = before.keys().chain(next.keys()).cloned().collect();
    let changed: Vec<_> = ids
        .into_iter()
        .filter(|id| before.get(id).map(&comparable) != next.get(id).map(&comparable))
        .collect();
    if changed.is_empty() {
        return replace(&paths.config, content.as_bytes());
    }
    for id in &changed {
        store.generations.insert(
            id.clone(),
            Generation {
                value: uuid(),
                ready: false,
            },
        );
    }
    commit(paths, &store)?;
    replace(&paths.config, content.as_bytes())?;
    for id in changed {
        store
            .generations
            .get_mut(&id)
            .expect("inserted generation")
            .ready = true;
    }
    commit(paths, &store)?;
    Ok(())
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
