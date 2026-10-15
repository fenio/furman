# Guarded S3 content writes

## Scope

Ordinary Copy from local/S3 to S3 and Sync with an S3 destination now prepare a
native, bounded write preview before approval. The preview checks actual S3
destination objects, not loaded panel rows. Sync retains its selected relative
paths and exclusions; SHA-256 comparison values are rechecked, never interpreted
as S3 ETags. Ordinary Copy is not automatically destination-verified; Sync is.

This is **not protection for every write API**. Moves, renames,
folder creation, SFTP/archive staging, OS drag/drop upload handlers, legacy
upload/copy commands, and client-side password-encrypted uploads retain their
existing behavior. Metadata edits have a [separate conditional-copy contract](s3-metadata-compatibility.md),
not the content-write preview/policy workflow described here. SSE-C is supported by the guarded path: original connection
clients/interceptors are preserved, with independent source/destination keys.
Keychain storage and HTTPS-only SSE-C transport are unchanged.

## Policies and approval

* **Create only** (default): reject a preview containing existing destinations;
  PUT and multipart completion send `If-None-Match: *`.
* **Skip existing**: omit objects that existed at preview time; missing objects
  still use create-only conditions, so a concurrent create is a conflict.
* **Replace if unchanged**: explicitly approve replacements; existing objects
  require a nonempty preview ETag and use `If-Match` on PUT/multipart completion.
  Missing objects remain create-only. An ETag is an opaque provider validator,
  not a payload hash or guarantee against metadata-only changes.
* **Overwrite**: separate, explicitly acknowledged, unconditional destination
  writes. The checkbox also acknowledges changes after the preview. Existing
  global overwrite preferences do not bypass this approval.

Previews are native, single-use, connection-bound and expire after 15 minutes.
Refreshing invalidates the previous preview and resets replacement approval.
At most four previews and 32 pinned local roots per preview are retained;
expired handles are pruned on the next preview insertion. Previews allow at
most 10,000 files, and share the configured recursive scan object/request/time
budgets, including local hashing and comparison rechecks. Incomplete previews
cannot authorize payload writes.

## Source and commit safety

Local preview reads use no-follow, capability-relative directory descriptors;
symlinks, non-UTF-8 filenames, and special files are rejected. A source's
SHA-256 is captured during preview. Immediately before writing each file, it
is reread into a private temporary snapshot and checked against that digest.
Only that snapshot is uploaded. Later source changes cannot change its payload.
Selected directory membership is fixed by the preview; newly added files are
not automatically included. Empty local directories are not uploaded.

S3 sources require ETags and use conditional GET into a private local snapshot.
The returned ETag and size must match. Sync also checks the approved SHA-256.
Guarded S3 copies always use GET/PUT relay, not destination-unconditional
CopyObject. This needs disk space for the largest selected object, reads and
uploads payloads through this machine, and may incur egress charges. Basic
content headers and user metadata are retained; this is not complete metadata,
tag, ACL, retention, storage-class, version-history, or bucket-config migration.

Write requests disable automatic SDK retries while retaining SSE-C and other
connection settings. Conditional failures, unsupported conditions, and transport
errors never cause an unguarded fallback. Failed multipart attempts are aborted
when the upload ID is known; cleanup failures are reported. A lost multipart
creation response may leave an unknown incomplete upload. Inspect the bucket's
incomplete uploads after uncertain outcomes.

These guarantees rely on providers correctly enforcing S3 condition headers.
Known unsupported Garage configurations are now blocked by a native safety gate;
see [provider restrictions](s3-metadata-compatibility.md#known-unsupported-destination-conditions).
Furman does not probe with scratch writes during read-only preview and cannot
prove that an arbitrary provider will not silently ignore protocol semantics.
Provider integration tests exercise conflicting creates/replacements; mocked
tests check exact PUT/completion conditions and unsupported-condition failures.

## Partial outcomes, cancellation, and retries

There is no multi-object transaction, snapshot isolation, or rollback. Completed
objects remain after a later conflict, error, or cancellation; an in-flight
request may have committed even if its response was lost. A failed attempt
consumes its approval. Inspect the destination and prepare/approve a fresh
preview; there is no automatic resume or reuse of stale approval.

Temporary snapshots are private and removed on normal completion/error, not
guaranteed securely erased or crash-recovered. Source snapshots and payload
transfers are cancellable but do not add a global transfer timeout or byte/disk
budget. Native local filesystem syscalls are cooperative between calls.
Sync follow-up deletions run only after every selected write succeeds with no
skips. Deletions themselves are not part of the conditional content-write
contract and must not be treated as transactional or revision-guarded.
