//! Explicit transport compatibility, never error-driven unguarded write retries.
use aws_sdk_s3::{
    config::RequestChecksumCalculation,
    error::{ProvideErrorMetadata, SdkError},
    types::ChecksumAlgorithm,
    Client,
};

pub fn upload_checksum(client: &Client) -> Option<ChecksumAlgorithm> {
    if matches!(
        client.config().request_checksum_calculation(),
        Some(RequestChecksumCalculation::WhenRequired)
    ) {
        None
    } else {
        Some(ChecksumAlgorithm::Crc32C)
    }
}

/// Only an explicit unsupported-operation response authorizes a different API.
/// Never retry access failures, bad signatures/checksums, transient failures,
/// unknown outcomes, malformed XML, or per-object errors as single deletes.
pub fn unsupported<E: ProvideErrorMetadata>(error: &SdkError<E>) -> bool {
    error
        .raw_response()
        .is_some_and(|response| response.status().as_u16() == 501)
        && error
            .as_service_error()
            .and_then(|service| service.code())
            .is_some_and(|code| {
                matches!(
                    code,
                    "NotImplemented" | "NotSupported" | "UnsupportedOperation"
                )
            })
}

pub fn content_type(key: &str, encrypted: bool) -> &'static str {
    if encrypted {
        return "application/octet-stream";
    }
    let name = key.rsplit('/').next().unwrap_or(key).to_ascii_lowercase();
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("txt" | "log" | "md" | "csv" | "tsv") => match name.rsplit('.').next().unwrap_or("") {
            "md" => "text/markdown",
            "csv" => "text/csv",
            "tsv" => "text/tab-separated-values",
            _ => "text/plain",
        },
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("js" | "mjs") => "text/javascript",
        Some("json" | "map") => "application/json",
        Some("xml") => "application/xml",
        Some("pdf") => "application/pdf",
        Some("yaml" | "yml") => "application/yaml",
        Some("toml") => "application/toml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("zip") => "application/zip",
        Some("gz" | "tgz") => "application/gzip",
        Some("tar") => "application/x-tar",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub fn validate_content_type(value: Option<&str>) -> Result<(), crate::models::FmError> {
    let Some(value) = value else {
        return Ok(());
    };
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    let media = value.split(';').next().unwrap_or("").trim();
    let parameters_valid = value.split(';').skip(1).all(|parameter| {
        parameter
            .trim()
            .split_once('=')
            .is_some_and(|(name, value)| {
                let value = value.trim();
                token(name.trim())
                    && (token(value)
                        || (value.len() >= 2
                            && value.starts_with('"')
                            && value.ends_with('"')
                            && !value[1..value.len() - 1].contains(['"', '\\'])))
            })
    });
    if value.len() > 256
        || !value.is_ascii()
        || value.bytes().any(|b| b.is_ascii_control())
        || !media
            .split_once('/')
            .is_some_and(|(kind, subtype)| token(kind) && token(subtype))
        || !parameters_valid
    {
        return Err(crate::models::FmError::S3("Invalid Content-Type: use a concrete media type such as application/json or text/plain; charset=utf-8".into()));
    }
    Ok(())
}
