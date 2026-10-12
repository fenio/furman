# S3 bucket migration and content verification

Open the source bucket/prefix in one pane and the destination in the other.

## Copying

- Ordinary folder copies keep the selected folder's name and nested paths.
- Bucket-to-bucket sync preserves paths relative to the chosen source prefix.
- Distinct source objects mapping to the same destination key are rejected before writing. Overlapping source/destination object sets on the same server are also rejected.
- Different configured S3 endpoints use download/upload relay through your machine. They must not attempt server-side copy against an unrelated bucket with the same name.
- Bucket-to-bucket **sync** hashes each copied source object and then reads and hashes its destination. A failed SHA-256 check fails the operation; the file is not marked complete and subsequent destination deletions are not run.
- Ordinary Copy does not automatically perform full content verification. Run a separate SHA-256 comparison afterwards.

## Verification

Use **Verify SHA-256** in the pane comparison bar, or select **SHA-256** in the Sync dialog and rescan. The Sync dialog defaults to SHA-256. Merely scanning does not modify either bucket.

Verification streams actual object bytes into SHA-256 on both sides. ETags are used only as conditional-read validators, never interpreted as MD5/SHA-256 digests. Multipart uploads, server-side encryption and provider-specific ETags do not cause size-only fallback. Modification timestamps are ignored in content mode. Metadata comparison remains available, but is explicitly labelled as not verifying contents and compares timestamps symmetrically.

Read failures, changed-object conditional-read failures and incomplete reads fail the scan. Cancelled/failed scans cannot claim equality or enable Sync. Changing comparison options invalidates the previous scan.

**Cost:** content verification downloads both sides in full. Verified copying also reads the source and destination. Budget for download/egress and request charges; server-side copy alone is cheaper.

## Safe migration procedure

1. Stop writers, or otherwise ensure the chosen source and destination remain stable throughout copying and verification. Listings plus GETs are not an atomic bucket snapshot.
2. Choose source and destination roots; run a SHA-256 scan.
3. Review selected new/modified files and any explicitly selected destination deletions. Sync is source-authoritative, not a bidirectional merge.
4. Run Sync. Do not delete source data when any operation failed, paused or was cancelled.
5. Run **Verify SHA-256** across both panes after completion. This mode includes all file names (unlike metadata pane comparison, which excludes `.DS_Store` and `Thumbs.db`). Clear any Sync-dialog exclusion patterns when checking a complete migration.
6. Confirm there are no missing or modified files; review destination-only files separately before removing the source.

## Scope and remaining work

This verifies current object payloads and paths, not a complete backup of an S3 bucket. Object versions, delete markers, tags, ACLs, storage class, lifecycle/replication configuration, encryption policy and metadata parity are not verified or guaranteed to migrate. Directory-marker objects ending in `/` are not included in comparison results. Application-encrypted objects are compared as stored ciphertext, not decrypted plaintext.

Keep buckets stable during verification; ETag conditional reads reduce races but do not provide snapshot isolation, especially on providers without usable validators. There is not yet a persistent, exportable SHA-256 migration manifest or an atomic rollback. Successful partial writes remain if a later file fails. Pause/resume checkpoints do not replace a final full-tree verification.

## Regression tests

The MinIO CI job runs Rust unit tests, the existing S3 integration suite, and `s3_migration`. The migration suite uses two isolated MinIO servers (`S3_TEST_ENDPOINT` and `S3_TEST_OTHER_ENDPOINT`) to test cross-endpoint copies where an unrelated source bucket has the same name on the destination server. It also covers nested paths, destination collisions, pause/resume, cancellation during verification, same-size content differences and identical content with different multipart ETags.
