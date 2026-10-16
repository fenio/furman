# Native read-only job binding and complete comparisons

Merged in [#364](https://github.com/fenio/furman/pull/364), this step extends the [definition foundation](saved-job-foundation.md) with
**explicit live binding and read-only SHA-256 comparisons**. Payload execution,
write previews, deletion, run history, restart replay and scheduling remain
disabled in that step. The [checked-preview follow-up](job-checked-previews.md)
now adds non-executable preparation and private snapshots. A complete manifest
is not an approval to write.

## Manual workflow

In Sync → Saved job definitions, select a definition whose configuration pins
match. The separate read-only comparison section uses the **saved definition
revision**, not unsaved editor fields, current pane roots or ordinary Sync
selections.

1. Review the saved source → destination, then approve **Bind native read-only
   session**. This reads only the required native OS keychain entries and sends
   up to two logical `HeadBucket` validations. No LIST/GET is sent during binding.
2. Review the resulting effective origins and budgets. Explicitly click
   **Compare saved job (SHA-256)**. Included S3 contents are downloaded through
   this machine for hashing; request and egress costs may apply.
3. A successful, exhaustive scan retains its manifest natively and displays
   summary counts. Differences are read in pages of 100 from that manifest.
   Destination-only entries are informational, never deletion instructions.
4. **Revalidate session** checks native ownership, expiry, definition/generation
   drift and roots without network/keychain reads. **Cancel / release session**
   invalidates its handle; selecting another definition or closing the dialog
   requests release and ignores late results. Starting over requires a new bind.

Listing definitions, selecting a job, opening the section and saving/rebinding
a definition still do not resolve credentials, connect or scan automatically.
Ordinary Sync and its guarded-write flow remain separate; job rows and private
client IDs cannot be passed into its legacy connection registry.
Ordinary Sync is temporarily disabled while the saved-job section is open, so
its separate read-only result cannot be mistaken for approval of a pane transfer.
Closing the section cancels/releases its native session without clearing the
ordinary pane comparison; returning to Sync is an explicit context switch.

## Native live-binding boundary

- Native code re-resolves the persisted saved profiles and exact job revision
  under the same metadata lock used by managed configuration/secret edits.
  It rejects stale generation markers, changed definitions and replaced local
  roots before reading keys. Required credentials, proxy passwords and independent
  SSE-C keys come from their existing native keychain entries, never IPC inputs.
  Missing/denied entries fail visibly without default-chain/anonymous fallback.
- Required keychain reads occur on a blocking worker while the identity lock
  protects the snapshot. The lock is released before any network await. A
  cancelled OS keychain prompt may still need to be approved/dismissed; once it
  returns, cancellation prevents additional key reads and bucket requests. No
  raw keychain errors/values are returned to the webview.
- Each side gets a new immutable native client-instance ID and private client.
  These clients are owned by the run, **not inserted into `S3State`**. SSE-C,
  explicit proxy authentication, provider condition gates, the signing backstop
  and safe activity observation remain attached. No keys or credential hashes
  are persisted in the job/session store or returned to the UI.
- Job clients are built deterministically with manual credentials, explicit
  region, explicit root endpoint and path-style requests. There is no AWS
  `ConfigLoader`, shared-profile discovery, credential process, IMDS, SSO, STS or
  implicit Express session lookup. Ordinary connection credential chains remain
  unchanged and available outside jobs.
- For declared AWS regional intent without a custom endpoint, supported region
  families map to an explicit HTTPS regional S3 origin, including China suffixes.
  Unsupported partitions/regions require an explicit endpoint. Acceleration and
  Express binding are not implemented; they fail rather than silently changing
  intent. AWS endpoint/FIPS/dual-stack/custom-CA environment overrides block
  binding. Standard OS/native TLS trust is used; this is not certificate pinning.
- A native interceptor validates the **resolved request URI** before signing and
  transmission against the pinned scheme, host and effective port. It also
  permits only `HeadBucket`, `HeadObject` (added for checked previews), `ListObjectsV2` and `GetObject` operations and only
  HEAD/GET HTTP methods. Namespace changes and mutation attempts fail before
  transmission. A stored endpoint string alone is not considered binding proof.
- Effective namespace overlap is checked again after resolution, so an implicit
  AWS regional origin and an explicit URL for that same origin cannot bypass
  prefix-overlap checks. Exact prefixes are not trimmed, URL-decoded or collapsed.
  Arbitrary DNS/proxy aliases, account ownership, principals and physical-server
  equivalence are not proved by this protocol. A signed `HeadBucket` is bucket
  access validation, not STS identity verification.

SSE-C is HTTPS-only. Keychain-enabled source/destination connections retain
independent keys, and the existing read-only plain-object SSE-C fallback is
preserved. SDK reads have at most two wire attempts, including that fallback;
budgets count logical SDK calls. This is not automatic job retry/resume. No
writer can weaken policy by using these private read-only clients: a future
approved writer needs a separately reviewed native derivation/authorization
path, not a frontend switch to strip the guard.

## Native comparison ownership

Only a run ID is accepted for comparison: there are no caller-supplied clients,
paths, hashes, comparison mode or `SyncEntry[]` parameters. One comparison can
start per bound run. Native code owns the definition revision, option snapshot,
reviewed global limits, effective limits, client instances and source descriptor.

Local enumeration and hashing are capability-relative to the **retained bound
root directory**, not independent reopens of a saved path. Every child open uses
no-follow behavior. Non-UTF-8 names, symlinks and special nodes fail closed;
excluded entries still count against enumeration budgets and are subject to
the node-type safety checks. File size/time/change metadata is checked across
streamed hashing. A replaced/renamed/unmounted root cannot silently become a
different comparison source; its reviewed location is revalidated at stage
boundaries and handle reads. File IDs remain drift indicators, not permanent
cryptographic identities.

S3 enumeration uses the existing shared scan budget and cursor checks, plus
strict object sizes, exact-prefix membership, duplicate-key rejection and
bounded opaque validators. Each included object's GET sends `If-Match`, requires
the listed response ETag and size, and hashes the streamed body with bounded
memory. Missing validators, changed ETags, malformed/incomplete bodies and failed
requests abort the scan. ETags are retained separately from SHA-256; they are
never interpreted as hashes. Empty folder markers are omitted; nonempty
slash-ending objects are rejected rather than silently ignored. Empty local
directories, metadata equality and version history are not compared.

Source/destination results are merged natively. Only exhaustive success after
final definition, root, limit and cancellation checks can publish a complete
manifest UUID. Retained entries contain native hashes/validators and native
entry IDs. Public pages contain only row ID, relative path, status and sizes;
they contain no private clients, hashes or reusable approvals. The run ID and
manifest ID must match the current complete native session. Cancelled, limited,
failed, expired, replaced or invalidated sessions cannot return manifest rows.

This is not snapshot isolation: files/objects can change during or after a scan.
Read-time conditions and hashes only describe the successful reads. The future
write-preview pipeline must recheck them and pin payload snapshots; no UI scan
state or manifest page may substitute for those native checks.

## Limits, cancellation and cleanup

- One active run and one complete manifest per application state, plus an
  exclusive nonblocking run lock across cooperating processes. A native task
  retains that lock until its network future/local worker has actually stopped,
  even after cancellation invalidates UI state.
- Binding has a 60-second cooperative budget and at most two logical bucket
  validations. Keychain/local filesystem syscalls cannot be forcibly interrupted;
  elapsed time/cancellation is checked before proceeding once they return.
- Bound sessions expire after 15 minutes; scanning/page/status reads do not
  slide expiry. Idle/native resources are reaped by a bounded cleanup timer, not
  a job scheduler. There is no startup work, approval restoration or network
  activity from expiry cleanup. A bounded 128-ID recent operation ledger also
  prevents duplicate/early-cancel binding IPC from becoming a new attempt.
- Comparison limits are the reviewed global budgets capped at 100,000 scanned
  entries and 900 seconds, shared across both sides. Input metadata has a 32 MiB
  estimated admission budget; at most 10,000 differences (including informational
  destination-only rows) are retained. Local relative paths/depth are capped at
  4,096 bytes / 128 directories. These are not an exact process-RSS limit.
- Changing limits or managed identity/definition data invalidates the run on
  the next native stage/handle check. Already-sent read requests may finish;
  incomplete results never become a complete manifest. No navigation-cache
  results enter this workflow. Partial progress remains visible for stopped
  comparisons, with explicit failed/cancelled/limited outcomes.
- Explicit cleanup failures are visible in the session UI. Close-time cleanup
  failures are logged as static, privacy-safe messages with native expiry as a
  backstop; late bind/scan/page responses cannot repopulate a closed or replaced
  component. No mutable SDK clients or root descriptors survive process exit.

## Diagnostics, validation and next gate

Long errors use the existing initially collapsed, copyable notices. Provider
permission errors retain their HTTP/code meaning and are not labeled expired
AWS SSO sessions. No bodies, signed URLs, authorization headers or keychain
output are copied into diagnostics. UI rows intentionally reveal relative file
names from the explicitly chosen roots; they are not persisted or added to the
privacy-safe activity export.

Tests cover real resolved-request write/origin guards, independent SSE-C clients
and plain-object fallback, exact prefixes, conditional GET response validation,
capability-relative reads, root replacement, input/difference budgets, missing
keys, pre-cancel/duplicate operations, single-flight locks, changed generations,
expiry, stale handles and cancelled/stalled reads. A loopback HTTP test exercises
the production job client, and an explicit disposable loopback MinIO test checks
native bind/compare without modifying either payload. The MinIO test uses
provider-supported fixture names; mocked protocol tests separately cover
literal repeated slashes. No real keychain entries or production buckets are
needed by these tests.

The [checked-preview follow-up](job-checked-previews.md) implements native
manifest-to-preview ownership and checked payload snapshots without execution.
The [journal foundation](job-journal.md) adds durable read-only attempt records,
restart reconciliation and review pauses. Single-use execution approval and
executable intent/outcome handling remain deferred. **No job write API or scheduling
is included.** Ordinary Copy/Sync, Linode/Akamai endpoints/proxy/SSE-C behavior and
the known Garage write gates are not reinterpreted by this read-only path.
