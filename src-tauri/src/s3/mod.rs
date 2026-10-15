pub mod client;
pub mod aws_profiles;
pub mod compat;
pub mod metadata;
pub mod activity;
pub mod crypto;
pub mod errors;
pub mod helpers;
pub mod scan;
pub mod service;
pub mod sse_c;
pub mod verification;
pub mod writes;
mod write_local;

#[cfg(test)]
mod listing_tests;
#[cfg(test)]
mod scan_tests;
#[cfg(test)]
mod compat_tests;

pub use client::{build_s3_client, S3Connection, S3State};
pub use helpers::{
    collect_local_files, copy_object_multipart, copy_single_or_multipart, list_all_objects,
    s3_path, s3err, strip_s3_prefix, throttle, upload_file_multipart, upload_part_with_retry,
    BANDWIDTH_LIMIT, COPY_MULTIPART_THRESHOLD, MAX_CONCURRENT_PARTS, MULTIPART_THRESHOLD,
    PART_RETRIES, PART_SIZE, PREVIEW_MAX_SIZE,
};
pub use service::S3Service;
