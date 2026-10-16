# Guarded S3 → S3 creates with native attribute preservation

This source/PR follow-up to [#367](https://github.com/fenio/furman/pull/367) extends
[manually approved job execution](job-guarded-execution.md) to eligible S3 sources.
It does not create a tagged release or enable scheduling. **Create-only / skip-existing,
5 MiB per file, 128 actionable native snapshots, single-use approval, durable
pre-write intent and mandatory SHA-256 verification remain unchanged.** There are
no replacements, multipart job writes, deletions, retries/resume or headless jobs.

## Supported attributes and explicit refusals

Native source GET captures the payload digest/size, opaque ETag, returned version
and bounded user metadata, Content-Type, Cache-Control, Content-Disposition,
Content-Encoding and Content-Language. These headers are carried into the
conditional PUT. A missing Content-Type, missing metadata indicators, partial
responses, unsupported headers or exceeded attribute bounds block execution;
values are not silently invented or truncated. Source storage must be STANDARD
(including the normal omitted STANDARD header), and destination STANDARD is explicit.

Execution review additionally requires source ACL, tag and bucket object-lock
reads. These are new read-only operations on the private native clients, not new
mutation capabilities. Preparation/comparison remain read-only and do not start
these attribute-control reads automatically.

- Only a recognized **owner-only** ACL is supported: one canonical FULL_CONTROL
  grant matching the nonempty source owner ID, or the exact policy-only owner
  stub used by MinIO. Missing/ambiguous owners, empty/custom/public ACLs and denied
  or unsupported ACL reads block approval; there is no unsupported/private fallback.
- The source owner is a native drift validator, not a grant to reproduce across
  accounts/providers. New destination ownership belongs to its own owner under
  normal default ACL/BucketOwnerEnforced semantics. No grant/ACL header is sent.
  Post-write verification requires a recognized owner-only destination ACL as well.
  Bucket policies can still grant access independently; they are not migrated or
  claimed private by these ACL checks.
- Tags are native-captured and sorted, then encoded on the single conditional PUT
  (`x-amz-tagging`), never written in a second unguarded tagging request. At most
  10 unique tags, nonempty keys up to 128 characters, values up to 256 characters
  and an encoded 8 KiB tag-header bound are admitted. A conservative portable
  character subset (letters/numbers/whitespace and `_ . : / = + - @`, no controls)
  is required. Unsupported tags refuse approval rather than being stripped or
  rewritten. The bounds are admission bounds after SDK decoding, not a claim to
  bound an adversarial server's raw XML transport allocation.
- **Object-lock-enabled source buckets are refused wholesale**, even if the GET
  appears to have no retention/legal hold. Those headers may be hidden without
  extra permissions. The native bucket inspection requires exactly HTTP 404 /
  `ObjectLockConfigurationNotFoundError` as explicit absence. A successful,
  ambiguous, denied, not-implemented or other missing response is not absence.
- KMS source encryption, non-STANDARD source storage, source retention/legal
  holds, Expires, website redirects and incomplete/unrepresentable attributes
  remain non-executable. Ordinary Copy/Sync is unchanged.

Source metadata admission is bounded to 16 KiB, relevant text headers to 4 KiB
each and version IDs to 1,024 bytes; no oversized value is retained for a writer.
Destination providers may impose stricter header/tag/policy limits. Their errors
stay visible and never cause an unguarded or attribute-dropping retry.

## Encryption and private relay

S3 sources still use GET → private unlinked plaintext snapshots → PUT. No
CopyObject API or shared source/destination key is introduced.

- SSE-S3 `AES256` sources request AES256 at the destination and verify it, unless
  the user explicitly selected an SSE-C destination: then its own key re-encrypts
  the plaintext. This choice and source encryption counts appear in review.
- Actual SSE-C sources require an explicitly SSE-C-configured destination. A
  destination with no SSE-C pin cannot authorize a plaintext downgrade.
- The source and destination clients retain **independent**, HTTPS-only,
  keychain-backed SSE-C keys. The writer derives only from the destination client.
  ACL/tag/object-lock requests receive no SSE-C key headers.
- Plain source objects use destination bucket encryption defaults. Destination
  bucket policy/lifecycle/retention defaults still apply; this is not a bucket,
  version-history, ownership, retention or KMS-key migration.
- No client-side password decryption/re-encryption is performed. Checksums describe
  the bytes returned by S3, and any preexisting client-encrypted payload/metadata
  remains those same bytes/metadata. Temporary plaintext staging and read/egress
  costs remain explicitly disclosed.

## Drift, versions and verification

During review, fresh conditional GET must match the prepared digest, size, ETag,
version and supported attributes. Native ACL/tag observations are retained in
the approval; their requests use the GET version ID when present. The tag response
must identify the same version, and a reported GET tag count must match the set.
Source owner IDs are hashed privately for drift checks, not exported or persisted.

Immediately before each mutation, repeat the current-source conditional GET,
object-lock inspection, ACL and tags, comparing all native observations to the
approved plan. A source version/header/tag/ACL change can invalidate approval even
when its ETag and SHA-256 remain the same. Snapshot bytes are separately rehashed;
no paths, keys, versions, hashes, metadata, tags or clients are accepted from IPC.

After the single `If-None-Match: *` PUT, the existing protocol first journals its
acknowledgement. Verification then requires returned ETag, current GET matching
the PUT version (also for versioned local-source creates), exact size, SHA-256,
supported headers and encryption. S3-source creates additionally require STANDARD
storage, matching tags and an owner-only destination ACL. No verified count is
recorded until **all** these checks complete. Denied/incomplete/mismatched
post-write checks are uncertain, never successful or rolled back; nothing retries
the PUT or mutates ACL/tags to repair it.

All reads, writes and verification share the reviewed execution request/object/
time budgets. Eligible S3 execution needs at least **nine logical calls per file**
(five source/destination preflight reads, PUT, payload verification and two
destination control reads); review itself uses five calls per file. A selection
whose known execution minimum exceeds the reviewed request budget is refused
before issuing approval. Existing retry-free mutation and bounded SDK read retry
rules remain; SDK wire retries can cost more than one logical call.

**No remote atomic metadata snapshot is promised.** ACLs and tags can change
independently of ETags and even versioned payloads, and source/destination state
can change after checks. The protocol preserves reviewed captured values and
rejects observed drift; it cannot freeze concurrent operators or bucket policies.
No source version is copied as a destination version ID. Configuration/generation/
origin checks still do not prove STS principal, bucket ownership, physical server
or DNS/proxy alias identity.

## Permissions, storage and validation

Read permissions now include source `GetBucketObjectLockConfiguration`, source
and destination `GetObjectAcl`/`GetObjectTagging` (version variants when needed),
plus normal comparison/HEAD/GET permissions. Tagged PUTs require appropriate
destination tagging permissions. Destination verification permissions cannot be
proved on a missing object: denial after PUT is an explicitly acknowledged
uncertain outcome, not a reason to claim success or drop attributes. Providers
that cannot establish source lock absence or expose ACL/tag state remain usable
for read-only jobs/ordinary workflows but cannot execute this cohort.

The existing v2 journal and restart protocol are unchanged. It records only safe
IDs/revisions/times/policy and aggregate counters, never object versions,
metadata/tag values, owner IDs/hashes, encryption keys, credentials or approvals.
Runtime review exports source encryption/tag **counts**, not raw control values.
No approval, SDK client or payload snapshot is restored on restart.

Native tests cover plain/AES256/SSE-C sources, independent connection-local keys,
versioned and unversioned controls, header/tag preservation, custom/ambiguous ACLs,
denials/unsupported controls, hidden lock hazards, portable/duplicate/oversized
tags, same-ETag source attribute/owner/version drift, verification failures and
insufficient request budgets. A disposable versioned/unversioned loopback MinIO fixture
verifies payload/metadata/headers/tags at both ends and an unchanged source version.
Existing local-create and read-only fixtures remain in MinIO CI. Tests do not use
production buckets or real keychain entries; real AWS/Linode/Akamai and native
live SSE-C cross-provider execution remain manually unverified.
