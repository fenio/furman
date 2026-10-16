//! Native destination observations for small conditional payload replacements.
//! Neither these validators nor attribute values are accepted from IPC/persisted.
use super::{
    attributes::Attributes,
    collect,
    controls::{self, Controls},
    error,
    preview::{Item, Policy},
    AtomicBool, Binding, BoundSource, FmError, ScanBudget,
};
use crate::s3::writes::DestinationExpectation;

pub(super) struct Destination {
    pub attributes: Attributes,
    pub controls: Option<Controls>,
}
pub(super) fn is_replacement(item: &Item) -> bool {
    matches!(item.expected, DestinationExpectation::Match { .. })
}
pub(super) fn blocker(item: &Item, policy: Policy, sse: bool, local: bool) -> Option<String> {
    if !is_replacement(item) {
        return if item.destination.is_some() {
            Some("Unexpected native destination attributes for a create".into())
        } else {
            None
        };
    }
    if policy != Policy::ReplaceUnchanged {
        return Some("Replacement requires the explicit replace-unchanged policy".into());
    }
    let Some(destination) = &item.destination else {
        return Some("Checked native destination attributes are missing".into());
    };
    if destination
        .attributes
        .metadata
        .keys()
        .any(|key| key.eq_ignore_ascii_case("furman-encrypted"))
    {
        return Some("Client-encrypted Furman destinations require a separate encryption workflow; payload replacement cannot retain/drop unrelated salt, nonce or password-encryption metadata.".into());
    }
    if local
        && destination
            .attributes
            .encoding
            .as_deref()
            .is_some_and(|encoding| encoding != "identity")
    {
        return Some("Local replacement cannot infer or transform a destination Content-Encoding; non-identity encodings remain unsupported.".into());
    }
    if item
        .entry
        .destination
        .as_ref()
        .is_none_or(|file| file.size > 5 * 1024 * 1024)
    {
        return Some(
            "Small-object replacements require both old and new payloads to be at most 5 MiB"
                .into(),
        );
    }
    if let Some(reason) = &destination.attributes.unsupported {
        return Some(format!("Destination is not safely replaceable: {reason}"));
    }
    if !destination.attributes.complete || destination.attributes.content_type.is_none() {
        return Some(
            "Destination attributes are incomplete; replacement cannot invent or drop them".into(),
        );
    }
    if destination.attributes.customer_encrypted && !sse {
        return Some("An SSE-C destination requires its own explicitly configured key; replacement cannot downgrade encryption".into());
    }
    None
}
pub(super) async fn check(
    binding: &Binding,
    item: &Item,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<Destination, FmError> {
    let DestinationExpectation::Match { etag } = &item.expected else {
        return Err(error(
            "Replacement requires its native opaque destination validator",
        ));
    };
    let expected = item
        .entry
        .destination
        .as_ref()
        .ok_or_else(|| error("Native compared destination is missing"))?;
    let captured = item
        .destination
        .as_ref()
        .ok_or_else(|| error("Prepared destination attributes are missing"))?;
    let (digest, attributes) = collect::hash_s3_with_attributes(
        &binding.destination,
        &item.key,
        expected.size,
        etag,
        cancel,
        budget,
        &|_| {},
    )
    .await?;
    if digest != expected.sha256 || attributes != captured.attributes {
        return Err(error("Destination payload, version or attributes changed after preparation; replacement approval cannot be reused"));
    }
    controls::require_unlocked_destination(&binding.destination, budget, cancel).await?;
    let controls = controls::read(
        &binding.destination,
        &item.key,
        attributes.version.as_deref(),
        budget,
        cancel,
    )
    .await?;
    if captured
        .controls
        .as_ref()
        .is_some_and(|expected| expected != &controls)
        || attributes
            .tag_count
            .is_some_and(|count| count as usize != controls.tags.len())
    {
        return Err(error(
            "Destination ACL/tags changed after review; no replacement was authorized",
        ));
    }
    Ok(Destination {
        attributes,
        controls: Some(controls),
    })
}
pub(super) fn write_attributes(binding: &Binding, item: &Item) -> Result<Attributes, FmError> {
    let mut attributes = if matches!(binding.source, BoundSource::Local(_)) && is_replacement(item)
    {
        item.destination
            .as_ref()
            .ok_or_else(|| error("Validated native destination attributes are missing"))?
            .attributes
            .clone()
    } else {
        item.snapshot.attributes.clone()
    };
    // S3 source headers/tags win, but a plain source never downgrades an AES256
    // destination. Configured destination SSE-C always uses its own private key.
    if item
        .destination
        .as_ref()
        .is_some_and(|dest| dest.attributes.aes256)
    {
        attributes.aes256 = true;
    }
    Ok(attributes)
}
pub(super) fn write_controls<'a>(binding: &Binding, item: &'a Item) -> Option<&'a Controls> {
    if matches!(binding.source, BoundSource::S3(_)) {
        item.controls.as_ref()
    } else {
        item.destination
            .as_ref()
            .and_then(|destination| destination.controls.as_ref())
    }
}
pub(super) fn minimum_calls(binding: &Binding, item: &Item) -> u64 {
    match (
        matches!(binding.source, BoundSource::S3(_)),
        is_replacement(item),
    ) {
        (true, true) => 12,
        (true, false) => 9,
        (false, true) => 8,
        (false, false) => 3,
    }
}
