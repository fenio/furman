# Pinned, manually launched sync jobs — design proposal

**Status: design merged in [#362](https://github.com/fenio/furman/pull/362);
definition-only foundation merged in [#363](https://github.com/fenio/furman/pull/363);
read-only live binding/comparisons merged in [#364](https://github.com/fenio/furman/pull/364);
checked non-executable previews/snapshots merged in [#365](https://github.com/fenio/furman/pull/365);
durable read-only journal/recovery merged in [#366](https://github.com/fenio/furman/pull/366);
initial guarded local-create execution is the current implementation step.** See the
[foundation contract](saved-job-foundation.md) for its exact scope and remaining
live-binding gates, and the [read-only runtime contract](job-bound-comparisons.md).
See [checked payload previews](job-checked-previews.md) for the implemented preparation boundary.
The [journal foundation](job-journal.md) implements read-only attempt durability,
local recovery and review pauses. The [initial execution cohort](job-guarded-execution.md)
adds native single-use approval, durable executable intent and verified outcomes
for local → S3 creates up to 5 MiB/file only. Broader S3-source, replacement and
multipart execution guarantees below remain unimplemented.
This document is the review gate between
the [implemented options presets](comparison-presets.md) and runnable jobs. It adds
no commands, execution paths, automatic connections, timers or migrations.
Merging this document does not enable runnable jobs or scheduling.

## Goal and first implementation scope

Save a named, directional definition, explicitly bind its identities for each
run, perform a fresh bounded comparison, review a native write preview, and
approve that attempt. Retain bounded, privacy-safe outcomes so interruption or
partial completion cannot look like success. A saved job is never an approval.

The first implementation should support **local → S3 and S3 → S3, copy/update
only**, using saved, keychain-backed manual S3 profiles. Restrict execution to
guarded S3 conditional-write semantics, with SHA-256 and post-write
verification required. Read-only comparisons remain available when destination
conditions are unsupported; they must not offer a runnable upload fallback.

This intentionally does **not** cover every ordinary Sync workflow:

The incremental executable cohort is narrower than the target scope above; see
[its explicit supported/excluded cases](job-guarded-execution.md).

- No deletion, move, rename, local destination, SFTP/archive destination,
  client-side password-encrypted upload, or bucket/version-history migration.
- No unguarded overwrite, even if the user previously approved it elsewhere.
- No default/environment credential chain, shared AWS profile/SSO,
  `credential_process`, anonymous auth, OIDC or assumed-role job execution until
  an effective-identity binding contract is separately implemented. Ordinary
  connections, SSO sign-in, Compare, Copy and Sync keep their current behavior.
- No automatic reconnect, retry, resume, startup execution, scheduler, daemon,
  headless/MCP job execution, or stored approval. A manual retry is a new run.

These are job-specific exclusions, not removal of Linode/Akamai, SSE-C, Garage,
local Sync, or AWS SSO support elsewhere in Furman.

## Why the current Sync dialog is not a job runner

The current boundaries matter:

| Existing code | What it provides | What a job must add |
| --- | --- | --- |
| `src/lib/components/SyncDialog.svelte` | Complete-only UI gating; changed options invalidate results; explicit Compare | Native ownership of the completed scan and selected entries |
| `src-tauri/src/commands/sync.rs` | Bounded, cancellable enumeration and comparison; streamed results | A retained, expiring complete-result handle bound to a job revision and actual endpoints |
| `src/lib/actions/sync.ts` | Dispatches caller-selected entries to several execution paths, including follow-up deletion | A dedicated copy-only job path that cannot dispatch deletes or legacy/unconditional writes |
| `src-tauri/src/s3/writes.rs` | Expiring, single-use, client-bound plans; conditional writes and checked snapshots | Job/run/scan ownership and job policy restrictions around the same native writer |
| `src-tauri/src/s3/client.rs` | Live client, bucket, region and SDK configuration | Immutable native connection generation and verified saved-profile configuration binding |
| `src/lib/types/index.ts` / saved connections | Frontend saved-profile IDs and connection metadata | Native resolution of profile identity; an ID or display label supplied by the UI is not proof |

Do not implement jobs as replayed frontend `sync-execute` events, saved
`SyncEntry[]`, or calls to the ordinary transfer dispatcher with stored paths.
The existing APIs remain available, but are not the authorization boundary for
the new job workflow. This design does not retroactively harden every legacy API.

## Persisted definition and identity contract

A versioned job definition contains only:

- UUID, unique name, monotonically changing definition revision and schema version;
- ordered source and destination pins, never “whichever pane is active”;
- explicit exclusions, SHA-256 mode, and a fixed `copy_update_only` operation kind.

Definitions must not persist runtime connection IDs, credentials or their hashes, encryption
keys or their hashes, authorization URLs/codes, SDK debug output, manifests,
selected files, write policy, approval tokens, or comparison results. An outcome
record may describe the policy used by a past attempt, but can never authorize
another attempt. Saving from a Sync dialog copies only the supported definition,
not its scan or selection.

### S3 pins

A pin identifies a saved profile UUID and opaque configuration generation,
exact bucket and key prefix, configured endpoint namespace, region, and transport
policy revision. The native layer must derive/validate the pin from the actual
connection and the saved profile, not trust these fields supplied by Svelte.

- Compare exact canonical endpoint scheme, host, effective port and supported
  base path; preserve distinctions between AWS regional endpoints and custom
  services. Do not infer AWS from a missing endpoint string. The native effective
  endpoint configuration, including SDK/environment overrides, must be accounted
  for. If its namespace cannot be determined safely, refuse to bind the job.
- Initially accept custom endpoint URLs with no userinfo, query, fragment or
  non-root base path. Do not redact a credential-bearing URL into a different
  endpoint and then treat it as the same identity. Ordinary custom connections
  with excluded URL forms continue to work outside jobs.
- Treat bucket/prefix as S3 names, not filesystem paths. Never URL-decode a
  prefix, collapse repeated slashes, resolve `.`/`..`, or silently remove a
  leading slash. Display the exact namespace and prefix at approval time.
  Reuse existing native key-mapping validation, including collision rejection.
- Keep direction immutable within a run. Reject equal or ancestor/descendant
  source/destination prefixes in the same endpoint-and-bucket namespace, using
  segment boundaries, not a bare string-prefix test. Do not assume that different
  profile IDs imply different namespaces. Known aliases must be treated as the
  same namespace and fail closed for known ambiguity. Configuration matching
  cannot prove that arbitrary DNS/proxy aliases point to distinct physical
  services; disclose that limitation rather than claiming universal overlap
  detection or probing with writes.
- Provider and transport intent, SSE-C enabled/disabled state, and keychain-entry
  generations participate in binding. Reuse the actual source/destination clients
  and independent SSE-C interceptors; never reconstruct a stripped-down client.
  SSE-C remains HTTPS-only, and no plain presigned links are introduced.

The required native saved-profile identity bridge is **not present today**.
The implementation must introduce immutable connection generations, native
resolution of a saved profile's allowlisted configuration, and random opaque
generation markers updated by supported keychain/configuration edits. Do not
derive a persistent identity fingerprint from access keys, passwords or SSE-C
keys, and do not duplicate secret values into job storage.

The bridge must invalidate affected job bindings **before** a managed identity
edit can take effect. Partial failure between keychain and configuration updates
leaves them unusable for jobs until explicitly rebound; it must not preserve an
apparently valid old generation. This is not a transaction spanning the OS
keychain and a JSON file. Existing live clients may remain usable by ordinary
workflows, but their old job generations cannot be reused.

Changing/deleting a saved profile, retargeting its endpoint/bucket, changing its
auth/encryption/transport intent, or changing its managed secret generation must
make a job **needs rebind**, not silently repoint it. Rebinding requires viewing
both roots and explicitly saving a new job revision; it never approves a run.
A display-name change need not invalidate identity, but must not identify a
profile by name. External keychain/provider changes may be outside Furman's
generation tracking: these are configuration pins, not proof of an AWS principal,
physical server identity, bucket ownership or unchanged remote content. State
that limitation; do not “verify” it by hashing credentials or making automatic
STS/probe requests against non-AWS providers.

### Local source pins

Save an absolute canonical UTF-8 directory path and platform filesystem/file-ID
metadata as drift checks. Reject symlink roots/components, special files and
unavailable roots; never substitute another directory, create a missing source,
follow a fallback bookmark or resolve it relative to the current pane.

At bind time reopen the root with no-follow, capability-relative operations and
retain the directory descriptor for the entire run. The comparison collector
and upload snapshot preparation must share that bound root, rather than reopen
the saved string independently. A replacement at the same path or a root
rename/unmount must invalidate execution if the saved location no longer refers
to the bound directory. This is stricter than ordinary pinned-source Copy.

File IDs are drift indicators, not permanent cryptographic identities: inode
reuse and remounted volumes must not be advertised as impossible. Require the
visible per-run root review, checked payload snapshots and a fresh comparison
regardless of the saved metadata. No local destination is authorized by this
proposal; it needs a separate guarded local-writer contract.

## Manual run protocol and authority

The following names describe **proposed native interfaces**, not current APIs:

1. **Save definition.** Validate the schema, supported profile types and root
   pins; perform no network requests and no credential resolution merely to list
   jobs. Job edits use compare-and-swap revisions and invalidate outstanding runs.
2. **Bind run.** Allocate a native `run_id` after the user reviews source →
   destination, and bind eligible live connections/root descriptors. If not
   connected, show an explicit connection
   action using existing keychain/SSE-C handling; never connect or sign in merely
   because a job was selected. Successful connection alone is not approval to
   scan or write. Capture native client generations and the reviewed scan limits.
3. **Compare.** An explicit action creates a native scan within the newly bound
   `run_id`, snapshotting job revision, roots, options and limits. Enumerate/hash
   without the navigation cache. Publish progress and costs; retain a complete manifest natively only
   after exhaustive success. Cancelled, limited and failed scans produce no
   usable completion handle. Destination-only entries are informational, not
   deletions. Equality/no-change is a terminal read-only result, not a write.
4. **Select and prepare.** The UI sends only run-scoped entry IDs from the retained
   manifest. Native code rejects duplicates, unknown paths, synthetic hashes,
   stale roots and destination-only entries. Never accept arbitrary replacement
   `WriteSelection` values from the UI as job authority. Recheck connection/root
   bindings and job revision, then create a fresh guarded write plan using the
   manifest's comparison expectations and the existing writer's checked sources.
5. **Approve and execute.** Present exact roots, selected counts/bytes, exclusions,
   conflicts, relay/download costs, verification and expiry. The user chooses
   `create_only`, `skip_existing` or explicitly acknowledged `replace_unchanged`.
   Native code disallows `overwrite` and requires verification, regardless of UI
   parameters. Consumption of a plan and transition to executing are atomic and
   single-use. Reject plans owned by another job/run/scan/client generation.
6. **Finish or stop.** Persist the outcome. A retry/rebind/rescan starts a new
   attempt and resets selection, policy and acknowledgements. No automatic
   continuation or replay after any terminal state, including app restart.

The native scan handle and write plan expire after at most 15 minutes. An
outstanding scan cannot be extended indefinitely by preparing another preview;
plan expiry is capped by the owning scan's expiry. A running payload operation
does not acquire a new global transfer deadline from this proposal.
An idle bound run also expires after 15 minutes without starting its scan;
expiry releases its clients, descriptors and run lock. Repeated UI refreshes do
not slide either deadline.

Initially allow one job run at a time across cooperating Furman processes using
a nonblocking advisory run lock, plus at most one complete retained manifest.
Cap each manifest at 10,000 actionable files and the existing scan budgets;
exceeding the job cap is an explicit incomplete/limited outcome, not implicit
batching. Retain the existing writer's local-root/preview limits. No mutex may
be held across network awaits; acquire immutable bound resources and perform
state transitions under short locks. Reject concurrent execute/cancel/rebind
races natively, including duplicate IPC and late channel events.

The job lock does not freeze other applications or ordinary Furman transfers.
ETag conditions and source snapshots remain necessary; this is not snapshot
isolation, an exactly-once protocol or a multi-object transaction. Destination
verification failures do not roll back objects already written.

## States, journal and restart behavior

Separate definition readiness (`ready`, `needs_rebind`, `unsupported`) from an
attempt's state:

```text
bound -> scanning -> awaiting_selection -> preparing -> awaiting_approval
                                                        -> executing -> succeeded
```

Branches are explicit: `no_changes`, `limited`, `failed`, `cancelled`, `skipped`,
`partial`, `uncertain` and `interrupted`. Do not call a run “synced” when it skipped
intended writes, failed verification, stopped after partial writes, or cannot
determine whether a request committed. Record confirmed completions separately
from attempted/unknown writes; cancellation is not proof an in-flight write
did not happen. Unsupported conditions, permission failures and expired SSO
sessions are distinct diagnoses, never reasons to weaken the write policy.

Persist an attempt record before starting its scan, then record stage changes
and its terminal outcome. Before the first payload mutation, durably record the run identity, definition
revision, chosen policy and `executing` intent, **without approval tokens**. If
that journal update fails, do not start writing. If persistence fails after any
write, retain a visible “outcome could not be recorded; inspect destination”
error and stop further job work. Never retry payloads to repair a journal error.
Storage failure must invalidate pending approval, not silently revert to
untracked execution. Mark the job paused for review after failure, partial,
uncertain, interrupted, or history-storage failure; require an explicit review
acknowledgement before a new run. A pause is not a scheduling capability.

On startup, nonterminal records become `interrupted` (with writes possibly
committed if execution had begun). Reconciliation is idempotent, performs no
network activity and restores no clients, descriptors, manifests or approvals.
If reconciliation cannot be saved, show the failure and block new job execution
until storage is usable. A restarted attempt always requires fresh binding,
comparison, preview and approval. A user can continue using ordinary workflows
while the job subsystem is unavailable.

Persist definitions and bounded outcome records in a versioned native store
separate from options presets and general preferences. Start with these limits:
100 definitions, 500 terminal attempts globally, 50 per job and 8 MiB total.
Evict oldest terminal records only; never evict an active intent to make a write
possible. If no safe space remains, refuse a new attempt with a visible error.
Deletion of a definition with an active run is refused; history removal requires
explicit confirmation and cannot conceal an unresolved active attempt.

Reuse the preset store's private permissions, bounded reads, strict schemas,
atomic replacement, cooperating-process locks and revision conflicts. Do not
treat malformed/future/unreadable storage as an empty store. The job journal
additionally requires reviewed file and directory durability semantics for
supported macOS/Linux filesystems; document platform/filesystem limitations and
test failures at every commit point. Existing preset atomic replacement alone
must not be advertised as a durable execution journal.

## UI, privacy and provider compatibility

- A Jobs view lists definitions, readiness and last outcome without connecting,
  scanning, resolving AWS credentials or launching CLI processes. Selecting a
  job shows full source → destination pins; no automatic fallback to active panes.
- Save, rebind, compare, prepare and execute are separate actions. Changing
  identity/options/limits invalidates scan/preview/approval. Keyboard activation,
  dialog close, navigation and late asynchronous replies cannot implicitly run a
  different job or reuse old approval. Closing a run view requests cancellation
  and explains that already-sent requests may still commit; journal ownership
  stays native until work stops. App exit must not mark it successful.
- Show scanning counts, S3 bytes hashed/downloaded, known transfer sizes,
  configured budgets and relay/egress costs. Skipped/incomplete/uncertain outcomes
  remain visible. Long diagnostics stay initially collapsed and copyable.
- Definitions contain sensitive *configuration* (paths, bucket names and endpoint
  namespaces) in a private plaintext file, not an encrypted vault. Disclose that.
  Do not store endpoints containing credentials or raw SDK/source-chain errors.
  Outcome records contain allowlisted job/run IDs, revisions, times, safe status
  categories, counters, policy and allowlisted provider status/code/request IDs;
  no object names, payloads, URLs, credential material or raw provider messages.
  Truncating a raw message is not sanitization. Existing activity export keeps
  its stricter allowlist; job definitions/history are not automatically included.
- Never add `aws sso login`, STS calls, scratch writes, optional-checksum changes,
  endpoint normalization or credential-chain resolution to profile/job listing.
  SSO usability and provider permission errors retain their existing distinction.
- Preserve custom-provider clients, proxies, SHA-256, independent SSE-C keys and
  conditional PUT/multipart completion. Known Garage conditional-write gates,
  including the signing backstop, stay native and survive all job client clones.
  A blocked Garage job remains blocked even though ordinary explicitly approved
  unguarded Copy is still available. No job policy silently selects that path.

## Implementation gates and acceptance tests

Implement in reviewable follow-ups, not by relaxing the proposal to fit the
current frontend dispatcher:

1. **Identity and definition foundation:** native saved-profile bridge,
   connection generations, root binding, strict schema and safe storage; UI can
   save/list/rebind definitions, but cannot execute them yet.
2. **Native manual run pipeline:** retained complete manifests, bounded run
   ownership, copy-only guarded plans, cancellation and durable intent/outcomes.
   Test the authorization boundary before exposing execution controls.
3. **Manual Jobs UI:** explicit fresh runs, approvals, history, review pauses and
   actionable diagnostics; provider and frontend CI must pass before enabling it.
4. **Separate later proposals:** broader auth/destination support and, only
   after manual reliability is established, opt-in scheduling. Scheduling needs
   its own unattended-approval, failure-pause, sleep/offline and missed-run design;
   neither this document nor merging manual jobs authorizes background writes.

Required tests before the first executable implementation ships:

- Same display name or bucket with a different profile/endpoint/port/scheme is
  not a match; missing/deleted/retargeted profiles, secret-generation edits,
  endpoint overrides and unknown effective identities fail closed. No secret
  material or secret-derived fingerprint is persisted or exported.
- Exact S3 prefix behavior (Unicode, repeated/leading slashes, literal `.`/`..`,
  adjacent-prefix names), self/overlap checks, alias ambiguity and mapping
  collisions; local symlink/root replacement, renamed roots, unmounts and
  source changes between scan, preview and payload snapshot.
- Cross-job/run/revision/client-generation handle misuse, synthetic entry IDs,
  duplicate execution, expiry, changed options/limits, concurrent IPC, cancelled
  or budget-limited scans, late events, and no use of navigation-cache results.
  Every rejected preflight sends zero payload writes and zero deletes.
- Create/replace races and unavailable conditional writes never downgrade;
  SHA-256 and post-write verification cannot be disabled via IPC. Source mutation
  or a destination conflict consumes approval and requires a fresh attempt.
- Exact PUT/multipart completion conditions, partial/skipped/uncertain outcomes,
  cancellation with a potentially committed request, multipart cleanup errors and
  no deletion callbacks. Test metadata/ETag limitations without promising more
  isolation than the existing writer provides.
- Keychain-backed Linode/Akamai configurations, AWS manual-key configurations,
  MinIO, SeaweedFS, Garage rejection, TLS SSE-C with independent keys and proxies;
  no production buckets/credentials in fixtures. Existing SSO/default-chain and
  ordinary non-job workflows retain their regression coverage.
- Storage revision conflicts, lock contention, permissions, corrupt/future/
  oversized/symlink files, full history, interrupted startup reconciliation and
  injected journal failures before and after writes. No auto retry/resume,
  accidental success, raw-message leakage or approval recovery after restart.
- Actual UI controller tests for save/rebind without execution, exact direction,
  keyboard behavior, changing panes while a run exists, closing during each
  phase, late completions, explicit review pauses and copyable error diagnostics.

This proposal is ready for review, **not evidence that any of these new
guarantees or acceptance tests have been implemented**.
