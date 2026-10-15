# S3 metadata edits and provider compatibility

## Conditional metadata edits

Properties loads a revision digest of the HEAD response and submits it with the
edited fields. Saving re-reads HEAD and rejects changed revisions before writing.
After saving, Properties reloads the metadata/revision; any failure invalidates
the draft's approval so the user must Reload before retrying. Reload explicitly
discards edits. Errors use the shared copyable, initially collapsed diagnostics.

Both small self-copies and multipart completion use destination `If-Match`.
CopyObject and every UploadPartCopy also carry `x-amz-copy-source-if-match`;
versioned sources include the HEAD version ID in the URL-encoded copy source.
ETags are opaque conditional validators, not content hashes. Legacy API and
batch callers without a loaded revision still get fresh native preflight and
conditional writes, but not a prior UI-snapshot comparison.

Objects at/above the configured upload multipart threshold (capped at 5 GiB)
use server-side multipart self-copy, including objects above 5 GiB. Parts are
at least 128 MiB, sized to stay within 10,000 parts and the 5 GiB part maximum.
Payloads are not downloaded. This incurs COPY requests and may incur storage
class/retrieval charges. Writes disable SDK retries and have no unconditional
fallback. Failed multipart copies/completions attempt bounded abort; cleanup
failure is reported. Unknown creation outcomes may leave incomplete uploads.
Normal copy semantics can change timestamps, version IDs, ETags and composite
checksum representation. Metadata edits do not perform a full payload SHA-256
download; the source is conditionally pinned and copied by the provider.

The four editable content headers and custom metadata map are replacements,
not a merge. Unedited readable attributes are carried forward:

* Content-Language, Expires, website redirect and storage class;
* object tags (Copy directive for small copies; version-pinned tag read for
  multipart creation);
* readable ACL grants; owner-only ACL headers are omitted for BucketOwnerEnforced;
* SSE-S3 AES256, SSE-C, retention mode/date and legal-hold status.

Reading ACLs, ownership controls and, for large edits, tags may require additional
permissions. Access-denied and unknown/malformed ACLs fail before writing, rather
than silently resetting grants. Explicit unsupported ACL APIs and MinIO's exact
anonymous canonical FULL_CONTROL ACL stub are recognized as policy-only/no-grant
cases. Normal S3 copy semantics still determine ownership of a new object/version;
this cannot preserve an arbitrary old owner's identity. Bucket policies/defaults
and lifecycle rules still apply.

SSE-C keys remain on their original connection and HTTPS-only. Metadata edits
through a keyed connection do not implicitly encrypt its plain objects. Reserved
Furman client-side encryption metadata cannot be removed/changed by this editor.
KMS/DSSE/custom encryption modes and event-based retention fail closed before
writing: their full context cannot yet be safely reproduced from HEAD. This is
deliberately safer than silently changing encryption or retention.

### Concurrency and outcome limits

There is no multi-attribute transaction or rollback. The revision check catches
loaded metadata changes visible in HEAD, but ACL/tag changes and metadata-only
races after preflight cannot all be atomically guarded. An ETag can remain
unchanged across metadata edits; unversioned Last-Modified can have coarse time
resolution. Providers must correctly enforce condition headers. Failed or lost
responses can have committed a new version; inspect/reload before retrying.
Metadata operations do not introduce a global transfer deadline or a new Cancel
command; existing SDK timeouts apply. Other metadata-mutating APIs such as
storage-class changes, restores and tag edits retain their separate contracts.

## MIME inference

Local payload uploads infer a conservative Content-Type from the destination
filename, for both single PUT and multipart creation, including guarded uploads.
Common text/web, JSON/YAML/TOML/XML, image, audio/video and archive extensions are
recognized case-insensitively. Unknown extensions use application/octet-stream.
This does not inspect file contents or infer Content-Encoding. Password-encrypted
payloads remain application/octet-stream. Guarded S3 relay copies retain their existing
source Content-Type instead of guessing from a new name.

Properties offers common Content-Type suggestions. Native validation rejects
missing type/subtype, wildcards, control characters, excessive length and malformed
simple parameters. Blank removes the explicit value and lets S3's defaults apply.
This is conservative media-type validation, not a full RFC MIME parser.

## Upload checksums

Configured custom endpoints use SDK RequestChecksumCalculation::WhenRequired,
including endpoints inherited from shared SDK configuration. Payload upload
builders do not explicitly demand optional CRC32C in that mode. Default AWS
behavior retains explicit CRC32C unless SDK configuration explicitly selects
WhenRequired. Model-required checksums/signing and SSE-C key-MD5 are unaffected;
providers with additional requirements can still reject a request visibly.

Server-side metadata copy does not force a multipart checksum algorithm: some
providers cannot supply it for encrypted UploadPartCopy. Returned part checksums
are forwarded at completion. No request is replayed with weaker checksums after
failure, and write guards are never removed. Transport checksums remain distinct
from Furman's SHA-256 payload comparison/Sync verification, which is unchanged.

## Bulk-delete fallback

Only a parsed HTTP 501 service response with code NotImplemented, NotSupported or
UnsupportedOperation triggers single-object deletion for that failed batch.
The complete bounded enumeration still finishes before any delete. Fallback
checks cancellation between objects and reports incremental completion counts.
It stops at the first single-delete failure and reports the partial count.
Bulk and fallback delete writes disable automatic SDK retries so an ambiguous
earlier attempt cannot be hidden behind a final unsupported-operation response.

There is no fallback for HTTP 400/403, signature/checksum errors, transient 5xx,
timeouts, malformed/unknown responses, or per-object errors in an HTTP 200 bulk
response. Those outcomes may be ambiguous or partial; silently replaying them
would be unsafe. Deletes remain current-key operations, not revision-guarded
transactions or a version-history purge. Rename/bucket-deletion internals retain
their existing delete behavior and do not use this fallback.
