//! Bounded native ACL/tag reads. Never serialized or accepted from callers.
use super::{error, job_sdk_error, AtomicBool, Endpoint, FmError, ScanBudget};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::operation::get_object_acl::GetObjectAclOutput;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq)]
pub(super) struct Controls {
    pub tags: BTreeMap<String, String>,
    // Source owner identity is a drift validator, never a destination grant.
    owner: String,
}
impl Controls {
    pub fn tagging(&self) -> Option<String> {
        if self.tags.is_empty() {
            return None;
        }
        Some(
            self.tags
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}={}",
                        urlencoding::encode(key),
                        urlencoding::encode(value)
                    )
                })
                .collect::<Vec<_>>()
                .join("&"),
        )
    }
}
pub(super) async fn require_unlocked_source(
    endpoint: &Endpoint,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<(), FmError> {
    require_unlocked(endpoint, budget, cancel, "source").await
}
pub(super) async fn require_unlocked_destination(
    endpoint: &Endpoint,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<(), FmError> {
    require_unlocked(endpoint, budget, cancel, "destination").await
}
async fn require_unlocked(
    endpoint: &Endpoint,
    budget: &ScanBudget,
    cancel: &AtomicBool,
    role: &str,
) -> Result<(), FmError> {
    // GET can hide per-object lock headers without GetObjectRetention/LegalHold
    // permission. Require explicit bucket-level absence, never infer it from a
    // payload response or an unsupported/denied request. Enabled buckets remain
    // non-executable even when a particular object appears unlocked.
    budget.request(cancel,async{
        match endpoint.client.get_object_lock_configuration().bucket(&endpoint.bucket).send().await{
            Err(failure) if failure.raw_response().is_some_and(|response|response.status().as_u16()==404)&&failure.as_service_error().and_then(|e|e.code())==Some("ObjectLockConfigurationNotFoundError")=>Ok(()),
            Err(failure)=>Err(job_sdk_error(&format!("Job {role} object-lock inspection (explicit absence is required)"),&failure)),
            Ok(_)=>Err(error(&format!("Object-lock-enabled or ambiguous {role} buckets cannot execute this operation: GET headers may hide retention/legal holds. No object-lock policy is dropped or bypassed."))),
        }
    }).await
}
fn owner_only(acl: &GetObjectAclOutput) -> Result<String, FmError> {
    let fail = || {
        error("Job source/destination ACL is not a recognized owner-only ACL. Custom grants, missing/ambiguous owners and unsupported ACL reads cannot authorize a payload-only copy; no grants were included in diagnostics.")
    };
    if acl.grants().len() != 1 {
        return Err(fail());
    }
    let grant = &acl.grants()[0];
    let owner = acl.owner().ok_or_else(fail)?;
    let grantee = grant.grantee().ok_or_else(fail)?;
    if grant
        .permission()
        .is_none_or(|p| p.as_str() != "FULL_CONTROL")
        || grantee.r#type().as_str() != "CanonicalUser"
        || grantee.uri().is_some()
        || grantee.email_address().is_some()
    {
        return Err(fail());
    }
    match (owner.id(), grantee.id()) {
        (Some(id), Some(grantee))
            if !id.is_empty()
                && id.len() <= 1024
                && !id.chars().any(|c| c.is_whitespace() || c.is_control())
                && id == grantee =>
        {
            Ok(Sha256::digest(id.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect())
        }
        // The exact policy-only ACL stub used by MinIO: one canonical FULL_CONTROL
        // entry, owner structure but no canonical IDs. Not an unsupported fallback.
        (None | Some(""), None) => Ok("policy-only-owner-stub".into()),
        _ => Err(fail()),
    }
}
pub(super) async fn read(
    endpoint: &Endpoint,
    key: &str,
    version: Option<&str>,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<Controls, FmError> {
    let acl = budget
        .request(cancel, async {
            endpoint
                .client
                .get_object_acl()
                .bucket(&endpoint.bucket)
                .key(key)
                .set_version_id(version.map(str::to_owned))
                .send()
                .await
                .map_err(|failure| {
                    job_sdk_error(
                        "Job ACL inspection (required; no unsupported/denied fallback)",
                        &failure,
                    )
                })
        })
        .await?;
    let owner = owner_only(&acl)?;
    let response = budget
        .request(cancel, async {
            endpoint
                .client
                .get_object_tagging()
                .bucket(&endpoint.bucket)
                .key(key)
                .set_version_id(version.map(str::to_owned))
                .send()
                .await
                .map_err(|failure| {
                    job_sdk_error(
                        "Job tag inspection (required; no unsupported/denied fallback)",
                        &failure,
                    )
                })
        })
        .await?;
    if response.version_id() != version {
        return Err(error("Job tag response does not match the native object version; no payload write was authorized"));
    }
    if response.tag_set().len() > 10 {
        return Err(error(
            "Object tags exceed the native 10-tag preservation bound",
        ));
    }
    let mut tags = BTreeMap::new();
    for tag in response.tag_set() {
        if tag.key().is_empty()
            || tag.key().chars().count() > 128
            || tag.value().chars().count() > 256
            || tag.key().chars().chain(tag.value().chars()).any(|c| {
                c.is_control()
                    || !(c.is_alphanumeric() || c.is_whitespace() || "_.:/=+-@".contains(c))
            })
            || tags.insert(tag.key().into(), tag.value().into()).is_some()
        {
            return Err(error("Object tags contain duplicate/oversized entries or characters outside the conservative portable set (letters, numbers, spaces and _ . : / = + - @); no values were included"));
        }
    }
    let controls = Controls { tags, owner };
    if controls.tagging().is_some_and(|value| value.len() > 8192) {
        return Err(error(
            "Encoded object tags exceed the bounded job header budget",
        ));
    }
    Ok(controls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::types::{Grant, Grantee, Owner, Permission, Type};
    #[test]
    fn owner_only_acl_acceptance_is_explicit_and_never_migrates_source_ids() {
        let owner = Owner::builder().id("source-owner").build();
        let grantee = Grantee::builder()
            .r#type(Type::CanonicalUser)
            .id("source-owner")
            .build()
            .unwrap();
        let grant = Grant::builder()
            .grantee(grantee)
            .permission(Permission::FullControl)
            .build();
        let acl = GetObjectAclOutput::builder()
            .owner(owner)
            .grants(grant.clone())
            .build();
        let fingerprint = owner_only(&acl).unwrap();
        assert!(!fingerprint.contains("source-owner"));
        let mut custom = acl.clone();
        custom.grants.as_mut().unwrap().push(
            Grant::builder()
                .grantee(
                    Grantee::builder()
                        .r#type(Type::Group)
                        .uri("http://acs.amazonaws.com/groups/global/AllUsers")
                        .build()
                        .unwrap(),
                )
                .permission(Permission::Read)
                .build(),
        );
        assert!(owner_only(&custom).is_err());
        let mut wrong = acl.clone();
        wrong.owner = Some(Owner::builder().id("someone-else").build());
        assert!(owner_only(&wrong).is_err());
        let mut ambiguous = acl.clone();
        ambiguous.owner = None;
        assert!(owner_only(&ambiguous).is_err());
        let stub = GetObjectAclOutput::builder()
            .owner(Owner::builder().build())
            .grants(
                Grant::builder()
                    .grantee(
                        Grantee::builder()
                            .r#type(Type::CanonicalUser)
                            .build()
                            .unwrap(),
                    )
                    .permission(Permission::FullControl)
                    .build(),
            )
            .build();
        assert_eq!(owner_only(&stub).unwrap(), "policy-only-owner-stub");
        let mut empty = stub.clone();
        empty.grants = None;
        assert!(owner_only(&empty).is_err());
    }
}
