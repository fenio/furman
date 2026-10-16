//! Private payload attributes, never serialized into job definitions/history.
use aws_sdk_s3::operation::get_object::GetObjectOutput;
use std::collections::HashMap;

#[derive(Clone, Default, PartialEq)]
pub(super) struct Attributes {
    pub metadata: HashMap<String, String>,
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub disposition: Option<String>,
    pub encoding: Option<String>,
    pub language: Option<String>,
    pub aes256: bool,
    pub customer_encrypted: bool,
    pub unsupported: Option<String>,
}
impl Attributes {
    pub fn local(key: &str) -> Self {
        Self {
            content_type: Some(crate::s3::compat::content_type(key, false).into()),
            ..Self::default()
        }
    }
    pub fn capture(response: &GetObjectOutput) -> Self {
        let metadata_bytes = response
            .metadata()
            .into_iter()
            .flat_map(|m| m.iter())
            .fold(0usize, |total, (key, value)| {
                total.saturating_add(key.len()).saturating_add(value.len())
            });
        if metadata_bytes > 16 * 1024
            || [
                response.content_type(),
                response.cache_control(),
                response.content_disposition(),
                response.content_encoding(),
                response.content_language(),
            ]
            .into_iter()
            .flatten()
            .any(|v| v.len() > 4096)
        {
            return Self{unsupported:Some("Object attributes exceed the bounded job-header contract; no values were retained".into()),..Self::default()};
        }
        let mut value = Self {
            metadata: response.metadata().cloned().unwrap_or_default(),
            content_type: response.content_type().map(str::to_owned),
            cache_control: response.cache_control().map(str::to_owned),
            disposition: response.content_disposition().map(str::to_owned),
            encoding: response.content_encoding().map(str::to_owned),
            language: response.content_language().map(str::to_owned),
            aes256: response
                .server_side_encryption()
                .is_some_and(|v| v.as_str() == "AES256"),
            customer_encrypted: response.sse_customer_algorithm().is_some(),
            unsupported: None,
        };
        let unsupported = response
            .server_side_encryption()
            .is_some_and(|v| v.as_str() != "AES256")
            || response.ssekms_key_id().is_some()
            || response.bucket_key_enabled() == Some(true)
            || response.object_lock_mode().is_some()
            || response.object_lock_legal_hold_status().is_some()
            || response.object_lock_retain_until_date().is_some()
            || response
                .storage_class()
                .is_some_and(|v| v.as_str() != "STANDARD")
            || response.tag_count().is_some_and(|n| n > 0)
            || response.expires_string().is_some();
        let text = value
            .metadata
            .iter()
            .map(|(k, v)| k.len() + v.len())
            .sum::<usize>();
        if unsupported {
            value.unsupported=Some("This source needs richer encryption/tag/retention/storage/expiry preservation than the initial job writer supports. No job write is available; ordinary workflows are unchanged.".into());
        }
        if text > 16 * 1024
            || value
                .metadata
                .iter()
                .any(|(k, v)| k.chars().chain(v.chars()).any(|c| c == '\r' || c == '\n'))
        {
            value.unsupported =
                Some("Source metadata exceeds the bounded safe job-header contract".into());
        }
        value
    }
}
