# Guarded multipart saved jobs

This source/PR extends [checked replacements (#369)](https://github.com/fenio/furman/pull/369)
to bounded larger local → S3 and S3 → S3 creates/replacements. It does not change
ordinary transfers, release versions, scheduling, deletion, dynamic AWS-auth jobs,
automatic retry/resume or headless/MCP execution.

## Practical scope

Saved jobs make repeatable archive uploads, deployment updates and cross-provider
copies reviewable: explicitly bind a saved direction, compare SHA-256 contents,
select exact differences, stage checked snapshots, review and approve once, then
verify destination bytes/attributes and retain durable outcomes. They are not a
backup system, atomic mirror, unattended replication or rollback feature.

Jobs now admit up to **1 GiB per new file and per compared old replacement file**.
Files through 5 MiB still use the existing single-shot conditional PUT. Larger
new payloads use sequential multipart staging with 8 MiB parts (last part may be
smaller), at most 128 parts/file. The existing 128 actionable files, 16 GiB total
snapshot ceiling and explicitly configured scan/request/time budgets remain.
Preparation still stages entire payloads as private `0600`, immediately unlinked
file capabilities, including decrypted SSE-C source bytes. Disk space is not
reserved; filesystem overhead is extra. This is not zero-disk server-side CopyObject.

Review rehashes snapshots in bounded chunks. Execution rehashes them again before
initiation, reads each part by offset from the owned capability (never a reopened
path), and independently hashes the exact bytes supplied to UploadPart. The full
digest/size must match before completion. One part buffer is at most 8 MiB;
SDK/network/hash buffers add overhead. No whole large payload is loaded in RAM.
Blocking readers retain native run ownership until they actually stop.

## Explicit approval and narrow writers

Multipart review shows file/part counts, exact mappings, the applicable attribute/
encryption policy, costs and cleanup limitations. `saved_job_execute` adds a
default-false `acknowledge_multipart`; old clients and ordinary create/replacement
approvals cannot silently authorize staging. Replacement acknowledgement remains
independent. Requests still carry native run/approval IDs and booleans only, not
caller-provided paths, validators, upload IDs, parts, clients or attributes.

Derived writers preserve connection-local proxy/SSE-C and provider condition
gates, replace only the private read-only namespace interceptor, disable SDK
mutation retries, and authorize exactly one transmission of one operation at the
approved origin/bucket/literal key. Initiation permits only `POST ?uploads`;
UploadPart permits its exact returned upload ID and canonical part number;
completion permits its exact upload ID and native destination condition;
abort permits only `DELETE` of that exact known upload. Unexpected/duplicate
query parameters, wrong operations/methods/origins/keys/IDs/part numbers,
weaker/missing/both conditions and replay fail before transmission.

CreateMultipartUpload carries the reviewed supported metadata, headers, tags,
STANDARD storage and AES256/SSE-C strategy from the existing contracts. Parts
retain independent destination SSE-C headers. The existing conservative transport
checksum policy and returned CRC32C part checksums are retained; these do not
replace mandatory full SHA-256 verification. No source ACL/owner ID is migrated.

After staging, source payload/attributes/ACL/tags and destination expectations
are checked again. Final `CompleteMultipartUpload` uses **If-None-Match: \***
for missing objects or the native compared **If-Match** ETag for replacements.
No unsupported condition, conflict or failed response causes unconditional
completion, a second completion/part/initiation, or a fallback PUT/CopyObject.
The initial non-sliding approval lease must admit the first mutation (initiation
for a multipart first file); then the worker owns the shared bounded execution
budget through cleanup/outcome recording. There is no sliding approval renewal.

Full conditional destination GET verification uses the completion ETag/version,
size, SHA-256, chosen headers and encryption; S3-source/replacement tag/ACL and
storage checks remain mandatory. Unsupported attributes appearing on a multipart
result fail verification. Durable acknowledgement and fully verified completion
are separate. ETags remain opaque validators, **not content hashes or an atomic
metadata/tag/ACL/version transaction**; the same-ETag race limitations and explicit
replacement disclosure from #369 still apply. No backup/repair/rollback is added.

For `n` parts the known minimum execution calls are `n+5` (local create),
`n+13` (local replacement), `n+15` (S3 create), `n+21` (S3 replacement).
Known insufficient selections fail before review reads/approval. SDK read retries
can consume extra wire requests. Cleanup has its own separate bounded allowance,
not an implicit extension of transfer authority or permission to publish bytes.

## Journal v4: private cleanup evidence

The journal remains private, bounded to 8 MiB, atomic and durably synced using
the existing storage/lock/barrier protocol. V1/v2/v3 histories are readable and
upgrade on the next commit with zero-default multipart fields; older readers
refuse v4. Legacy schemas cannot smuggle multipart authority. Selected multipart
counts are immutable; original attempted/acknowledged/verified/uncertain totals
and replacement subcounts still describe whole objects, not successful parts.

**Privacy change, separately disclosed in multipart approval:** a native-only
`uploads` ledger inside `~/.config/furman/job-runs/journal.json` retains the exact
destination origin, bucket, key and bounded returned provider upload ID, linked
to a random evidence ID/run. These are cleanup evidence, not restored capabilities.
Never stored: credentials, encryption keys, payloads, source locations, metadata/
tag values, ETags/hashes, per-part validators, clients or execution approvals.
Normal IPC history/global notices expose evidence IDs, phases, known/unknown ID
status and part attempt/receipt counts only—not private locations/upload IDs.

Durable barriers occur **before initiation**, after its returned upload ID and
before any part, before every part with its exact next attempt number, after each
bounded part receipt, before completion, and before cleanup. A storage failure
halts subsequent mutation/cleanup; a returned ID whose commit failed is not
invented as durably tracked. A lost/malformed initiation receipt leaves explicit
unknown-ID evidence: no listing guess, initiation replay or speculative abort.
Evidence is removed only following a completion receipt or eligible confirmed
abort/explicit `404 NoSuchUpload` receipt, with its own durable commit.

Known uploads receive abort-only cleanup after staging/completion failure or
cancellation: at most **three single-shot attempts within a shared 30-second
cleanup budget**. Cleanup ignores user cancellation and exhausted transfer budgets,
but never changed native identity/origin or broken journal durability. The worker
retains the native lease until cleanup/outcome work actually ends. Cancellation
does not guarantee rollback; a transmitted completion may already have committed.

**Abort acknowledgement is insufficient when a transmitted part lacked a durable
receipt:** an in-flight part can finish after abort. Even a successful abort then
retains unresolved evidence and an uncertain outcome for independent inspection.
Denied/timed-out/ambiguous cleanup likewise retains evidence and visible storage-
charge warnings. No ListParts/ListMultipartUploads recovery, credentials or clients
are restored at startup; recovery only marks active attempts interrupted and
preserves the exact last durable phase/part counters.

At most 128 unresolved upload records are admitted globally. They and their
attempts cannot be evicted by retention or cleared as history, even after ordinary
job-pause review. After manually inspecting/resolving provider incomplete uploads
(or lifecycle cleanup), a **separate revision-checked, explicitly acknowledged
external-cleanup review** can remove a selected native evidence ID's private
location/ID and increment its attempt's audit count. This action is local-only,
requires exclusive idle ownership/healthy recovered storage, and accepts no caller
locations/upload IDs. It is a **user attestation, not native cleanup verification**;
original failed/uncertain transfer outcomes and review pauses remain unchanged.
Wrong/stale/unknown/duplicate/live reviews fail closed. It authorizes no reconnect,
abort, retry, resume or transfer. Do not manually delete/edit the journal to hide
uncertain resources. Provider bucket lifecycle rules are recommended as a separate
operator-configured safety net; Furman does not install them automatically.

## Validation boundary

Replay/fault tests cover bounded part bodies, local/S3 create/replacement attribute
and AES256/SSE-C policies, separate approval, exact conditional writers, single use,
known-minimum budgets, post-staging destination changes, failed parts, bounded
cleanup, lost initiation receipts, completion 412/501 and bad SHA-256 verification,
snapshot mutation, cancellation, durable pre-transmission phase/part intent, and
post-response journal faults before further mutation. Journal tests cover legacy
migration, strict/private evidence, retention and local-only interrupted recovery/
external-review audit. UI tests cover independent acknowledgement and local review.

Disposable loopback MinIO fixtures exercise real multipart local/S3 creates and
replacements in versioned/unversioned buckets, including old payloads above 5 MiB,
and check SHA-256-backed outcomes, metadata/tags, unchanged source versions and
empty incomplete-upload inventories. This is not live AWS/Linode/Akamai or native
cross-provider SSE-C validation; those remain manually unverified. No production
credentials, buckets or real keychain entries are used.
