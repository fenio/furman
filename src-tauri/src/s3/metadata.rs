//! Conditional, attribute-preserving metadata self-copies. No GET/PUT relay,
//! unconditional fallback, or retry after an uncertain write outcome.
use super::{errors::s3_sdk_error, helpers::get_multipart_threshold};
use crate::models::FmError;
use aws_sdk_s3::{
    operation::head_object::HeadObjectOutput,
    types::{CompletedMultipartUpload, CompletedPart, MetadataDirective, TaggingDirective},
    Client,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

fn invalid(message: impl Into<String>) -> FmError {
    FmError::S3(message.into())
}

pub fn revision(head: &HeadObjectOutput) -> String {
    let metadata: BTreeMap<_, _> = head
        .metadata()
        .into_iter()
        .flat_map(|values| values.iter())
        .collect();
    let snapshot = serde_json::json!({ "metadata": metadata, "etag": head.e_tag(), "version": head.version_id(),
        "size": head.content_length(), "modified": head.last_modified().map(ToString::to_string),
        "type": head.content_type(), "disposition": head.content_disposition(), "encoding": head.content_encoding(),
        "language": head.content_language(), "cache": head.cache_control(), "expires": head.expires_string(),
        "redirect": head.website_redirect_location(), "storage": head.storage_class().map(|value| value.as_str()),
        "encryption": head.server_side_encryption().map(|value| value.as_str()), "bucket_key": head.bucket_key_enabled(),
        "retain_until": head.object_lock_retain_until_date().map(ToString::to_string),
        "lock": head.object_lock_mode().map(|value| value.as_str()), "legal_hold": head.object_lock_legal_hold_status().map(|value| value.as_str()) });
    Sha256::digest(snapshot.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Canonical grant headers, or no headers for the normal owner-only ACL. This
/// also works with BucketOwnerEnforced, where sending grant headers is forbidden.
async fn grants(
    client: &Client,
    bucket: &str,
    key: &str,
    version: Option<&str>,
) -> Result<HashMap<&'static str, String>, FmError> {
    use aws_sdk_s3::error::ProvideErrorMetadata;
    let acl = match client
        .get_object_acl()
        .bucket(bucket)
        .key(key)
        .set_version_id(version.map(str::to_owned))
        .send()
        .await
    {
        Ok(acl) => acl,
        Err(error)
            if super::compat::unsupported(&error)
                || error.as_service_error().and_then(|error| error.code())
                    == Some("AccessControlListNotSupported") =>
        {
            return Ok(HashMap::new())
        }
        Err(error) => {
            return Err(s3_sdk_error(
                "Read ACL before metadata edit (required to avoid resetting grants)",
                &error,
            ))
        }
    };
    // MinIO's documented ACL stub has no owner/grantee identifiers and exactly
    // one canonical FULL_CONTROL entry. It represents policy-only access, not
    // a grant that can be reproduced. Do not generalize this to empty/custom ACLs.
    if acl.owner().is_some()
        && acl
            .owner()
            .and_then(|owner| owner.id())
            .filter(|id| !id.is_empty())
            .is_none()
        && acl.grants().len() == 1
        && acl.grants()[0]
            .permission()
            .is_some_and(|permission| permission.as_str() == "FULL_CONTROL")
        && acl.grants()[0].grantee().is_some_and(|grantee| {
            grantee.r#type().as_str() == "CanonicalUser"
                && grantee.id().is_none()
                && grantee.uri().is_none()
                && grantee.email_address().is_none()
        })
    {
        return Ok(HashMap::new());
    }
    if acl.grants().len() == 1
        && acl.grants()[0]
            .permission()
            .is_some_and(|permission| permission.as_str() == "FULL_CONTROL")
        && acl.grants()[0].grantee().and_then(|grantee| grantee.id())
            == acl.owner().and_then(|owner| owner.id())
        && acl.owner().and_then(|owner| owner.id()).is_some()
    {
        match client
            .get_bucket_ownership_controls()
            .bucket(bucket)
            .send()
            .await
        {
            Ok(ownership)
                if ownership.ownership_controls().is_some_and(|controls| {
                    controls
                        .rules()
                        .iter()
                        .any(|rule| rule.object_ownership().as_str() == "BucketOwnerEnforced")
                }) =>
            {
                return Ok(HashMap::new())
            }
            Ok(_) => {}
            Err(error)
                if super::compat::unsupported(&error)
                    || error
                        .as_service_error()
                        .and_then(|error| error.code())
                        .is_some_and(|code| {
                            matches!(
                                code,
                                "OwnershipControlsNotFoundError" | "NoSuchOwnershipControls"
                            )
                        }) => {}
            Err(error) => {
                return Err(s3_sdk_error(
                    "Read ownership controls before preserving the owner ACL grant",
                    &error,
                ))
            }
        }
    }
    let mut headers: HashMap<&'static str, Vec<String>> = HashMap::new();
    for grant in acl.grants() {
        let header = match grant.permission().map(|permission| permission.as_str()) {
            Some("FULL_CONTROL") => "full",
            Some("READ") => "read",
            Some("READ_ACP") => "read_acp",
            Some("WRITE_ACP") => "write_acp",
            _ => {
                return Err(invalid(
                    "Cannot preserve an unknown ACL permission; metadata was not changed",
                ))
            }
        };
        let grantee = grant
            .grantee()
            .ok_or_else(|| invalid("ACL grant has no grantee"))?;
        let (kind, value) = if let Some(id) = grantee.id() {
            ("id", id)
        } else if let Some(uri) = grantee.uri() {
            ("uri", uri)
        } else if let Some(email) = grantee.email_address() {
            ("emailAddress", email)
        } else {
            return Err(invalid("Cannot preserve an unknown ACL grantee"));
        };
        if value.is_empty()
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || b"\"\\".contains(&byte))
        {
            return Err(invalid("Cannot safely encode the object's ACL grants"));
        }
        headers
            .entry(header)
            .or_default()
            .push(format!("{kind}=\"{value}\""));
    }
    if headers.is_empty() {
        return Err(invalid(
            "Provider returned an empty ACL; cannot safely preserve permissions",
        ));
    }
    Ok(headers
        .into_iter()
        .map(|(name, values)| (name, values.join(",")))
        .collect())
}

pub async fn update(
    client: &Client,
    bucket: &str,
    key: &str,
    content_type: Option<&str>,
    disposition: Option<&str>,
    cache: Option<&str>,
    encoding: Option<&str>,
    custom: &HashMap<String, String>,
    expected_revision: Option<&str>,
) -> Result<(), FmError> {
    super::compat::validate_content_type(content_type)?;
    super::compat::check_conditional_writes(client)?;
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|error| s3_sdk_error("Read metadata before update", &error))?;
    if expected_revision.is_some_and(|expected| expected != revision(&head)) {
        return Err(invalid("Object changed since metadata was loaded. Reload metadata and review your edits; nothing was written."));
    }
    let size = head
        .content_length()
        .filter(|size| *size >= 0)
        .ok_or_else(|| invalid("Object has no usable size; metadata was not changed"))?
        as u64;
    let etag = head
        .e_tag()
        .filter(|etag| !etag.is_empty())
        .ok_or_else(|| {
            invalid("Object has no ETag; conditional metadata editing is unavailable")
        })?;
    if let Some(original) = head
        .metadata()
        .filter(|metadata| metadata.contains_key("furman-encrypted"))
    {
        for name in [
            "furman-encrypted",
            "furman-kdf",
            "furman-salt",
            "furman-nonce",
            "furman-original-size",
            "furman-kdf-m",
            "furman-kdf-t",
            "furman-kdf-p",
        ] {
            if original.get(name) != custom.get(name) {
                return Err(invalid("Client-side encryption metadata cannot be removed or changed by the metadata editor; nothing was written"));
            }
        }
    }
    if head
        .server_side_encryption()
        .is_some_and(|algorithm| algorithm.as_str() != "AES256")
    {
        return Err(invalid("Cannot safely preserve this encryption mode during metadata edits. KMS/custom encryption context is not fully readable via HEAD; metadata was not changed."));
    }
    if head.object_lock_event_hold().is_some() {
        return Err(invalid(
            "Event-based object retention cannot yet be safely preserved; metadata was not changed",
        ));
    }
    let expires = head
        .expires_string()
        .map(|value| {
            aws_smithy_types::DateTime::from_str(
                value,
                aws_smithy_types::date_time::Format::HttpDate,
            )
            .map_err(|_| {
                invalid("Provider returned an invalid Expires header; refusing to discard it")
            })
        })
        .transpose()?;
    let permissions = grants(client, bucket, key, head.version_id()).await?;
    let source = format!(
        "{}/{}{}",
        bucket,
        urlencoding::encode(key),
        head.version_id()
            .map(|version| format!("?versionId={}", urlencoding::encode(version)))
            .unwrap_or_default()
    );
    let client = super::sse_c::metadata_client(client, head.sse_customer_algorithm().is_some());
    let client = Client::from_conf(
        client
            .config()
            .to_builder()
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
            .build(),
    );
    // Source and destination checks are both required: a pinned version is not
    // permission to replace a newer current payload. ETags are not content hashes.
    if size == 0 || size < get_multipart_threshold().min(super::helpers::COPY_MULTIPART_THRESHOLD) {
        client.copy_object().bucket(bucket).key(key).copy_source(source).copy_source_if_match(etag).if_match(etag)
            .metadata_directive(MetadataDirective::Replace).tagging_directive(TaggingDirective::Copy)
            .set_content_type(content_type.map(str::to_owned)).set_content_disposition(disposition.map(str::to_owned))
            .set_cache_control(cache.map(str::to_owned)).set_content_encoding(encoding.map(str::to_owned))
            .set_metadata(Some(custom.clone())).set_content_language(head.content_language().map(str::to_owned))
            .set_expires(expires.clone()).set_website_redirect_location(head.website_redirect_location().map(str::to_owned))
            .set_storage_class(head.storage_class().cloned()).set_server_side_encryption(head.server_side_encryption().cloned())
            .set_object_lock_mode(head.object_lock_mode().cloned()).set_object_lock_retain_until_date(head.object_lock_retain_until_date().cloned())
            .set_object_lock_legal_hold_status(head.object_lock_legal_hold_status().cloned())
            .set_grant_full_control(permissions.get("full").cloned()).set_grant_read(permissions.get("read").cloned())
            .set_grant_read_acp(permissions.get("read_acp").cloned()).set_grant_write_acp(permissions.get("write_acp").cloned())
            .send().await.map_err(|error| s3_sdk_error("Conditional metadata copy (no unconditional fallback; inspect destination before retrying)", &error))?;
        return Ok(());
    }
    let tags = client
        .get_object_tagging()
        .bucket(bucket)
        .key(key)
        .set_version_id(head.version_id().map(str::to_owned))
        .send()
        .await
        .map_err(|error| {
            s3_sdk_error(
                "Read tags before multipart metadata edit (required to preserve tags)",
                &error,
            )
        })?;
    let tagging = tags
        .tag_set()
        .iter()
        .map(|tag| {
            format!(
                "{}={}",
                urlencoding::encode(tag.key()),
                urlencoding::encode(tag.value())
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    let created = client.create_multipart_upload().bucket(bucket).key(key)
        // This is server-side UploadPartCopy, not a streamed payload upload:
        // do not force a checksum algorithm that encrypted copy parts cannot
        // supply on some providers. Preserve any checksums the provider returns.
        .set_content_type(content_type.map(str::to_owned)).set_content_disposition(disposition.map(str::to_owned))
        .set_cache_control(cache.map(str::to_owned)).set_content_encoding(encoding.map(str::to_owned))
        .set_metadata(Some(custom.clone())).set_content_language(head.content_language().map(str::to_owned))
        .set_expires(expires).set_website_redirect_location(head.website_redirect_location().map(str::to_owned))
        .set_storage_class(head.storage_class().cloned()).set_server_side_encryption(head.server_side_encryption().cloned())
        .set_object_lock_mode(head.object_lock_mode().cloned()).set_object_lock_retain_until_date(head.object_lock_retain_until_date().cloned())
        .set_object_lock_legal_hold_status(head.object_lock_legal_hold_status().cloned())
        .set_grant_full_control(permissions.get("full").cloned()).set_grant_read(permissions.get("read").cloned())
        .set_grant_read_acp(permissions.get("read_acp").cloned()).set_grant_write_acp(permissions.get("write_acp").cloned())
        .set_tagging(if tagging.is_empty() { None } else { Some(tagging) })
        .send().await.map_err(|error| s3_sdk_error("Create multipart metadata edit", &error))?;
    let upload_id = created.upload_id().ok_or_else(|| {
        invalid("Metadata multipart response has no upload ID; inspect incomplete uploads")
    })?;
    let result = async {
        let part_size = (128 * 1024 * 1024u64).max(size.div_ceil(10_000));
        if part_size > 5 * 1024 * 1024 * 1024 { return Err(invalid("Object exceeds supported multipart copy limits")); }
        let mut parts = Vec::new();
        for number in 0..size.div_ceil(part_size) {
            let start = number * part_size;
            let end = (start + part_size).min(size) - 1;
            let response = client.upload_part_copy().bucket(bucket).key(key).upload_id(upload_id).part_number(number as i32 + 1)
                .copy_source(&source).copy_source_if_match(etag).copy_source_range(format!("bytes={start}-{end}"))
                .send().await.map_err(|error| s3_sdk_error("Copy part for metadata edit", &error))?;
            let part = response.copy_part_result().ok_or_else(|| invalid("Metadata copy part has no result"))?;
            let etag = part.e_tag().ok_or_else(|| invalid("Metadata copy part has no ETag"))?;
            parts.push(CompletedPart::builder().part_number(number as i32 + 1).e_tag(etag)
                .set_checksum_crc32(part.checksum_crc32().map(str::to_owned)).set_checksum_crc32_c(part.checksum_crc32_c().map(str::to_owned))
                .set_checksum_sha1(part.checksum_sha1().map(str::to_owned)).set_checksum_sha256(part.checksum_sha256().map(str::to_owned))
                .set_checksum_crc64_nvme(part.checksum_crc64_nvme().map(str::to_owned))
                .set_checksum_sha512(part.checksum_sha512().map(str::to_owned)).set_checksum_md5(part.checksum_md5().map(str::to_owned))
                .set_checksum_xxhash64(part.checksum_xxhash64().map(str::to_owned)).set_checksum_xxhash3(part.checksum_xxhash3().map(str::to_owned))
                .set_checksum_xxhash128(part.checksum_xxhash128().map(str::to_owned)).build());
        }
        client.complete_multipart_upload().bucket(bucket).key(key).upload_id(upload_id).if_match(etag)
            .multipart_upload(CompletedMultipartUpload::builder().set_parts(Some(parts)).build())
            .send().await.map_err(|error| s3_sdk_error("Conditional multipart metadata completion (no unconditional fallback; inspect destination before retrying)", &error))?;
        Ok(())
    }.await;
    if let Err(error) = result {
        let abort = client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .send();
        if !matches!(
            tokio::time::timeout(std::time::Duration::from_secs(30), abort).await,
            Ok(Ok(_))
        ) {
            return Err(invalid(format!("{error}\nCould not confirm multipart cleanup; inspect incomplete uploads at the destination.")));
        }
        return Err(error);
    }
    Ok(())
}
