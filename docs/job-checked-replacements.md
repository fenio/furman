# Checked small-object job replacements

This source/PR follow-up to [S3 creates (#368)](https://github.com/fenio/furman/pull/368)
enables `replace_unchanged` for selected local → S3 and S3 → S3 payload differences.
Both **old and new replacement payloads must be at most 5 MiB**; 128 actionable
snapshots and the original non-sliding session lease still apply. No release/tag,
multipart job writes, deletion, unguarded overwrite, retry/resume or scheduling.

## Native approval and conditions

Preparation now retains destination attributes from its existing conditional
SHA-256 GET, without another request or an old-payload backup. The saved native
manifest owns destination size, real SHA-256, opaque ETag and exact mapping.
Source snapshot capabilities remain private `0600`, unlinked and rehashed.

Execution review and the actual execution independently recheck destination
SHA-256, size, ETag, version, supported attributes, owner-only ACL and portable
tags. Object-lock-enabled or ambiguously inspected destination buckets are
refused wholesale, including hidden retention/legal-hold cases. Source checks
and preservation gates from [the S3 create contract](job-s3-create-execution.md)
remain unchanged. Unsupported KMS, storage, expiry/redirect, incomplete headers,
custom ACLs or denied/unsupported control inspection cannot authorize replacement.

The native review shows every mapping as create or replacement, policy/revision,
replacement and encryption counts, attribute strategy and exclusions. Existing
objects require a **third explicit acknowledgement** (`acknowledge_replacements`)
in addition to execution and exclusion approval. Its default is false, so old
create-only IPC clients cannot authorize replacements accidentally. Requests
still accept only native IDs and acknowledgement booleans, never destination
paths, clients, hashes, validators, metadata or writer handles.

After single-use approval consumption and durable attempt intent, each write
derives an exact per-key/origin/bucket conditional writer. Missing destinations
use `If-None-Match: *`; existing destinations use exactly their native compared
`If-Match` ETag and no `If-None-Match`. Wrong/missing/both/weaker conditions fail
before transmission. Mutation retries remain disabled; a scoped writer cannot
transmit twice. Garage condition gates and connection-local SSE-C/proxy handling
remain attached. No CopyObject or unguarded fallback is introduced.

## Attribute and encryption policy

- **Local replacement:** keep checked destination user metadata, Content-Type,
  Cache-Control, Content-Disposition, Content-Language and supported Content-Encoding,
  plus its native-reviewed tags. Local MIME inference applies only to new objects.
  Non-identity Content-Encoding is refused for local replacements: no compression
  transform is inferred from arbitrary local bytes.
- **S3 replacement:** reviewed source headers/metadata/tags replace destination
  values, as explicitly disclosed. This is not a metadata merge. Source attributes
  still have to pass their native preservation/ACL/retention/encryption gates.
- Existing destination AES256 is retained even when the source is plain, unless
  its explicitly configured destination SSE-C key re-encrypts it. Existing SSE-C
  destinations require that independent connection-local key; no plaintext
  downgrade or source/destination key sharing is authorized.
- Known `furman-encrypted` client-encrypted destinations are non-executable for
  replacement; unrelated salt/nonce/password-encryption metadata cannot be retained
  over new local bytes or silently discarded. No password encryption migration.
- Only recognized owner-only ACLs are supported. New object/version ownership
  follows destination rules, not migration of the previous object's owner ID.
  Bucket policies and stronger destination defaults are not migrated or frozen.

The final GET requires the PUT version, size, SHA-256, chosen supported headers,
encryption and STANDARD storage. ACL/tag verification must finish before any
verified count advances. Unsupported/unreviewed replacement attributes appearing
after PUT are uncertain, not successful; there is no repair, rollback or tagging/
ACL mutation outside the original conditional PUT.

**Important race limit:** `If-Match` protects an opaque ETag, not a SHA-256 or
atomic metadata/ACL/tag/version transaction. Those properties may change without
changing an ETag, between or after the rechecks. Observed drift refuses writes,
but concurrent same-ETag metadata/control edits after the last check can be
overwritten. Approval explicitly acknowledges this limitation. This is not a
backup/rollback feature or proof that a destination cannot change after verification.
Configuration/origin checks remain neither STS principal nor bucket ownership/
physical-server/DNS/proxy identity proof.

## Budgets and durable outcomes

Known minimum execution calls are three for a local create, nine for an S3
create, **eight for a local replacement and twelve for an S3 replacement**.
Selections involving S3 sources/replacements whose known minimum exceeds the
reviewed request budget are refused before approval. Review costs four calls per
local replacement and eight per S3 replacement. Existing SDK read retries can
cost more wire requests; PUT is never retried. Filesystem work remains cooperative.

Journal **v3** reads v1/v2 histories with zero-default replacement subcounts and
atomically upgrades on the next commit. Older readers refuse v3. Records add
planned/attempted/acknowledged/verified/uncertain replacement counts to the existing
total counters; v1/v2 cannot smuggle replacement intent, and create policies cannot
carry nonzero replacement authority. Selection counts are immutable and progress
is monotonic. Every attempt/subcount is durable before its PUT, acknowledgement
and complete verification are separate commits, and terminal durability is mandatory
for success. No paths, ETags, hashes, attributes, owner IDs or approvals are persisted.

Restart marks unfinished attempts interrupted, preserving verified totals and
replacement subcounts while conservatively marking outstanding attempts unknown.
It restores no approvals/clients/manifests/snapshots and performs no retry.
Cancellation, conflict/unsupported responses, lost responses, failed verification
and post-write journal faults remain visible partial/uncertain outcomes with
explicit review pauses. A sent replacement may have committed; cancellation is
not rollback. Existing global monitoring, cache invalidation, run-lease ownership,
retention limits, private storage barriers and initially collapsed/copyable
diagnostics remain in force.

Tests cover independent local/S3 attribute policies, AES256/SSE-C retention,
separate acknowledgement, single use, exact signed conditions, same-ETag payload/
version/header/ACL/tag drift, unsupported destination attributes/controls,
transmitted 412/501 errors, verification failure, v3 counter validation and legacy/
interrupted reconciliation. Disposable loopback MinIO tests perform real local/S3
replacements in versioned and unversioned buckets and verify attributes/tags and
unchanged sources. No production buckets, credentials or real keychain entries
are used; real AWS/Linode/Akamai/native live cross-provider SSE-C remain manually
unverified. Scheduling and broader write cohorts still require separate review.
