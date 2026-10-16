# Initial manually approved job execution

Merged in [#367](https://github.com/fenio/furman/pull/367), this follow-up to [checked previews](job-checked-previews.md) and the
[#366 journal foundation](https://github.com/fenio/furman/pull/366) enables a
deliberately narrow first executable cohort. **Definitions, comparisons,
preparation, startup/recovery and history review are never execution approval.**
This is source/PR functionality; it does not create a new tagged release.
The [S3-source follow-up](job-s3-create-execution.md) extends the same guarded
cohort to supported S3 metadata/headers/tags/encryption with explicit ACL/lock gates.
The [replacement extension](job-checked-replacements.md) adds checked small-object
`replace_unchanged`, a third acknowledgement and durable replacement subcounts.

## Supported cohort and explicit exclusions

- Saved manual-key **local → S3**, plus [eligible S3 → S3](job-s3-create-execution.md)
  jobs; SHA-256 is mandatory.
- Explicit native selected files, at most 128 actionable snapshots, each **up to
  5 MiB**. No folder expansion or silently included unloaded entries at execution.
- `create_only` or `skip_existing`: every actionable destination must still be
  missing, and every PUT uses `If-None-Match: *`. Selected existing files skipped
  at preparation are reported as skipped, not transferred or called synced.
- Destination root or normal folder prefixes; leading/repeated slashes, dot
  segments, percent/backslash/control characters remain non-executable rather
  than being normalized into a different object key. Existing read-only previews
  still retain exact supported S3 keys.
- Local bytes plus inferred MIME only. No local ownership/mode, filesystem ACL,
  xattrs or timestamps are migrated. No fabricated tags, object ACL, storage class
  or retention settings. New objects inherit destination bucket defaults unless
  destination SSE-C is explicitly configured.
- Destination SSE-C retains its **own** connection-local HTTPS-only keychain key,
  native SSE interceptor and mandatory encrypted verification. Secrets never
  enter IPC or the journal. Proxy/routing configuration is inherited from the
  native bound destination client, not resolved again through a credential chain.

Binding retains its [existing identity limitations](job-bound-comparisons.md):
configuration/generation/root pins and signed bucket access are not STS principal
verification, proof of bucket ownership or physical-server/DNS/proxy alias identity.
Managed credential edits invalidate generations before mutation; external keychain
edits and editors ignoring locks remain outside that tracking.

**Not enabled:** unguarded replacements, larger-object/multipart
job writes, unsupported S3 source attributes, deletion/move/rename, local destinations,
unguarded overwrite, dynamic AWS profile/SSO execution, retry/resume, headless/MCP
execution or scheduling. Custom source ACLs, KMS/retention and non-STANDARD source
storage remain refused, never a payload-only downgrade. Larger files and richer
policies visibly remain read-only. Ordinary guarded Copy/Sync is unchanged.

## Manual flow and native ownership

1. Explicitly bind the saved revision, compare SHA-256, select native difference
   entry IDs and prepare checked unlinked payload snapshots as before.
2. For eligible previews only, click **Recheck for execution review (reads only)**.
   Native code consumes the prepared plan, records `reviewing`, checks every
   destination is missing, rehashes current retained-root local sources and
   rehashes snapshot capabilities with bounded memory. It revalidates identities,
   root metadata, client instances, manifest/preview/run IDs and original expiry.
3. Native code durably records `awaiting_approval` and returns a random runtime-only
   single-use approval ID. The UI shows saved revision, exact roots/effective
   destination, policy, exclusions, all mappings, selected/skipped counts, bytes,
   destination encryption, non-sliding expiry, preservation exclusions and costs.
4. Separately acknowledge exclusions/partial-outcome risks and every exact
   mapping. **Execute approved creates once** sends only native run/approval IDs
   and the two explicit acknowledgement booleans. Unknown fields, caller-supplied
   paths, clients, keys, hashes, sizes, validators and policy overrides are refused.
5. Native code atomically consumes approval and admits one owned execution task.
   It cannot be replayed, including after cancellation/errors or lost IPC replies.
   The original run lease must be fresh at admission and immediately before the
   first PUT. After the first mutation is admitted, the worker retains the run
   lease to record outcomes even when the original approval deadline passes.
   Execution has a separate **shared**, non-sliding bounded request/object/time
   budget using the reviewed native limits, not a fresh allowance for each file.

Before each PUT, identities/current source/missing destination and retained
snapshot bytes are checked again. The body is read by offset from its private
unlinked native file capability, hashed and materialized in at most 5 MiB memory;
no source pathname, `/dev/fd` path or ordinary `WriteState` approval is reopened.
This also avoids shared-offset replay bugs from cloned file descriptors. Native
plan ownership supplies every key, expected digest/size, MIME and policy.

A private per-key client is derived from the native bound client only after
approval consumption and durable attempt intent. The read-only guard is replaced
by a guard for exactly **PutObject / PUT / the approved origin, bucket and literal
key / If-None-Match: \***. Unexpected operations, keys, methods, origins, query
parameters or weaker conditions fail before signing/transmission. SDK mutation
retries are disabled and a second transmission through that scoped writer is
refused. Provider condition safety, SSE-C and diagnostic interceptors remain.
Declared Garage and Garage-format key-ID gates still fail closed; this does not
claim that arbitrary custom providers honor conditional writes.

The separate native task owns outcome completion, not the IPC caller or view.
Closing the dialog requests cancellation; it cannot revoke a sent request.
Cancellation/status reads never mark executing work terminal prematurely or
release its worker lease. Dropped execution tasks conservatively record
`interrupted`; restart never restores approvals, clients, manifests or snapshots.
Native read-only clients stay unable to write. No executable client handle is
exported to ordinary transfers or frontend code.

## Durable executable intent and mandatory verification

Journal **v2** can read v1 records with absent new fields, and upgrades atomically
on its next commit using the existing private flush/dirty-marker protocol. An
older reader refuses v2 rather than misreading executable state as read-only.
Marker contents are unchanged fixed non-secret protocol evidence. Existing
definitions and identity generations are not migrated or rebound automatically.

The native sequence is:

```text
prepared → reviewing → awaiting_approval → executing
          (reads only)                    ↳ succeeded / skipped
                                         ↳ failed / cancelled / partial / uncertain
```

- Durably record selected file/byte counts, skipped counts and the prepared
  create policy before entering any mutation stage.
- Durably increment an object's attempted-intent count **before transmission**.
  An attempt is conservative evidence, not proof a request reached the server.
- After a successful PUT response, separately record acknowledged count before
  continuing. No PUT response alone is called verified or successful.
- Require a bounded returned ETag and conditional GET, exact size and real
  SHA-256 of all bytes. Verify MIME when set and supported payload headers and
  destination SSE-C behavior. ETags are opaque conditional validators only.
- Durably record verified file/byte counts before the next object. No new PUT
  starts after journal failure, cancellation, identity drift or budget failure.
- Persist the terminal outcome. `succeeded` requires every selected actionable
  object fully verified and no skips/unknown writes. `skipped` separately reports
  intentional skipped selections. `partial` distinguishes confirmed completed
  objects from later failures. Any transmitted error, timeout/cancellation,
  unpinned/incomplete/mismatched verification or unknown response is conservatively
  `uncertain`, even if a provider error might represent a clean rejection.

Journal faults after a write preserve in-memory confirmed counters, but **never
report durable success**: live status becomes uncertain with `outcome_recorded:
false` and copyable storage/operation diagnostics. The last durable intent remains
the restart backstop. A terminal update may already have committed even when its
final flush failed. Recovery marks nonterminal attempts interrupted, preserving
verified counts and marking outstanding attempted-but-unverified objects unknown;
it never retries to discover whether they committed. Failed, limited, partial,
uncertain and interrupted attempts require explicit review before another run.
No automatic rollback or deletion is attempted.

History retains only allowlisted IDs/revisions/times/policy, read-stage counters
and selected/attempted/acknowledged/verified/uncertain aggregates—never object
names/paths/endpoints, hashes/ETags, payloads, encryption keys, credentials or
approval IDs. It is not a per-object resume log. Following an uncertain restart,
inspect the intended destination and explicitly make a fresh comparison.
Storage retention, private permissions, exclusive recovery and failure barriers
remain as documented in [the journal foundation](job-journal.md).

## UI visibility and validation

Only an explicit execution action starts runtime-only local status monitoring;
startup/history remain local recovery only, with no reconnection/replay. Monitoring
survives closing the original dialog and retains global outcomes/errors. A lost
status response does not retry execution: inspect history/destination. Normal S3
display caches are conservatively invalidated before admission and after the
observed outcome, including failure, with cache admission suspended while monitored.
An unavailable monitor does not make display caches authoritative; refresh after
inspecting the native outcome. Native jobs never authorize operations from caches.

Tests cover native ownership, single use, explicit acknowledgements, expiry,
source/snapshot/destination drift, signed per-key writer guards, SSE-C header
retention, durable pre-transmission intent, post-PUT/post-verification journal
faults, cancellation after PUT, SHA-256 failure, no mutation retries, partial and
skipped outcomes, v1→v2 migration, interrupted count reconciliation and UI
late-reply/global visibility. The disposable loopback MinIO fixture verifies a
real create and a destination appearing after review without overwriting it;
provider CI retains existing MinIO/SeaweedFS/Garage/SSE-C regression checks.
No production credentials, keychain entries or buckets are used for validation.

The [S3 extension](job-s3-create-execution.md) implements the supported preservation
subset. Future expansion requires separate review for custom grants, KMS/retention,
conditional replacement, multipart ambiguity/cleanup and their
failure-injection/provider matrix. Scheduling remains separately authorized.
