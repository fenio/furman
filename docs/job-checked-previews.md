# Checked, non-executable job previews

Merged in [#365](https://github.com/fenio/furman/pull/365), this follow-up to [native comparisons](job-bound-comparisons.md) implements the
manifest-to-preparation boundary and private payload snapshots. **No job writes,
execution approval, restart replay or scheduling are enabled.**
The preview is not inserted into ordinary `WriteState` and cannot be consumed by
ordinary Copy/Sync. All native job clients still reject payload mutation.
The [journal follow-up](job-journal.md) now records read-only attempts and adds
local restart reconciliation/review pauses; it does not add executable intents.

## Explicit manual flow

After binding and successfully comparing a saved job, select individual loaded
native difference rows. Destination-only rows cannot be selected. No select-all
action silently includes unloaded rows. Then review:

- `create_only`: reject a selection containing an existing destination;
- `skip_existing`: omit/report selected existing destinations; stage only new
  files. An all-skipped selection creates no preview;
- `replace_unchanged`: check existing destinations against the comparison's
  opaque ETag, size **and actual SHA-256**, retaining the conditional expectation.
  Choosing this policy is not approval to execute replacements.

Review the temporary disk budget (default **1,024 MiB**, UI range 1–16,384 MiB)
and explicitly approve additional reads/downloads and temporary plaintext only.
**Prepare checked preview (no uploads)** starts preparation; opening the view,
selecting entries and changing policy/budget do not start it. Selection, policy
and budget changes reset the UI acknowledgement. Execution remains unavailable
regardless of policy, UI state or returned preview ID.

Preparation may download included S3 source contents and replacement-destination
contents again. Request/egress costs apply. Included source bytes are retained
as plaintext even when fetched with SSE-C. Private unlinked temporary files are
not an encrypted vault or guaranteed secure erasure (OS swap/backups/filesystem
behavior remain outside the guarantee).

## Native authority and lifecycle

`saved_job_prepare` accepts a strict request containing only `run_id`,
`manifest_id`, native `entry_ids`, allowlisted policy, `max_snapshot_bytes` and
explicit read/staging approval. There are no caller paths, client IDs, hashes,
opaque validators, destination keys, verification flags or synthetic selections.
Unknown fields and `overwrite` are rejected by native deserialization.

Only a fresh complete manifest owned by that exact bound run can admit
preparation. Native code rejects unknown/cross-run/duplicate IDs,
destination-only rows, invalid local relative mappings, destination key
collisions/oversize, unsupported conditional providers and invalid budgets.
These selection errors cause no network requests or source snapshots and leave
the comparison available for an explicitly corrected selection. Validation may
read native configuration/root metadata, but never navigation-cache results.

Admission atomically changes `complete` → `preparing`, preventing concurrent
preparation/selection reuse. A successful attempt changes to `prepared` and
retains one native plan. Errors, cancellation, limits or interrupted tasks after
admission invalidate the comparison and release all partial snapshots; a new
bind/compare is required. No automatic retries/resume of preparation occur;
the existing bounded SDK read retry/plain-object SSE-C fallback remains intact.

The plan pins the job ID/revision, run ID, manifest ID, immutable client-instance
IDs, reviewed policy, native selection and destination expectations. All checks
are revalidated at admission/stage boundaries and immediately before publication.
The deadline is exactly the existing session's deadline, never a new 15-minute
lease. Prepared handle/status reads recheck native binding and plan ownership.
Changed managed generations, profiles, job revisions, limits, roots or client
instances invalidate it. A public preview UUID is neither authorization nor an
ordinary guarded writer handle.

Cancel/release/expiry/closing the saved-job section discards the plan and its
unlinked file descriptors. A running local/disk worker retains the native run
lock until it actually finishes, even if its asynchronous caller is dropped.
Already-sent reads and blocking local filesystem calls cannot be forcibly
recalled; late UI replies/progress cannot restore cancelled state. There is no
durable approval or snapshot restoration after process exit.

Identity and run-lock guards explicitly unlock at final owner drop rather than
relying only on descriptor close. This prevents a short-lived fork-inherited
duplicate (before child exec closes it) from spuriously retaining a finished
operation's lock. Worker `Arc` leases still prevent early unlock while I/O runs;
the duplication and final-owner behavior have deterministic regression tests.

## Destination checks and source snapshots

Before allocating any source snapshot, recheck every included destination:

- HEAD of a compared missing key must return a recognized missing-key/not-found
  404. Permission errors and arbitrary 404 provider errors are not absence.
- HEAD of an existing key must return the comparison's opaque ETag and size.
  A conditional GET must then return that same ETag/size and hash to the exact
  native comparison SHA-256. Missing, changed or truncated responses fail closed.
- Selected entries skipped by `skip_existing` are explicitly counted, not read
  or snapshotted and not represented as completed writes.

Local sources are opened capability-relative to the **same retained no-follow
root used by comparison**, using the shared native local-reader primitive.
Symlinks/special files and replaced roots cannot redirect snapshot reads.
Snapshotting streams bytes, checks regular-file type, size, SHA-256 and
size/time/change metadata across the read. A same-size content change cannot
silently pass as a match. File IDs/metadata are not principal/content proof.

S3 sources are read from the exact saved prefix plus native relative key,
without path normalization. GET sends the compared opaque `If-Match` validator,
requires the compared response ETag/length, streams into the private snapshot
and checks actual size and SHA-256. ETags are never content hashes. Independent
source/destination SSE-C keys, explicit proxies, origin guards and known Garage
condition gates remain attached. The read-only guard now also permits
`HeadObject`; PUT/COPY/DELETE/multipart mutations remain forbidden.

Snapshots use private `0600`, immediately unlinked temporary files. They are
explicitly chmodded owner-only before any payload is written, including on Linux
where unnamed `O_TMPFILE` allocations otherwise inherit broader umask-based modes. They are
retained only as native capabilities, never returned as filesystem paths or
registered for generic file/transfer IPC. A changed live source after successful
preparation cannot change the retained bytes. Disk worker ownership is retained
through dropped futures. Snapshot data/digests/ETags/credentials are not serialized
into reports, configuration, definitions, activity exports or a new journal.

This is not snapshot isolation: destinations can change after their checks, and
the comparison is not a globally atomic listing. A future executable writer must
recheck native ownership/expectations, rewind and verify snapshot consumption,
preserve supported source attributes/encryption semantics and enforce conditional
PUT/multipart completion. These payload-only previews do not claim complete
metadata/ACL/tag/retention/version migration.

## Budgets and diagnostics

- One preview per run; at most **128 actionable file capabilities**. Comparison
  manifests retain their existing 10,000-difference cap, but preparation is
  deliberately smaller to bound descriptors. Skipped entries need no capability.
- Native disk budget range: 1 byte–16 GiB. Sum all included source sizes with
  overflow checks **before** source I/O. A stream growing beyond its compared
  size is rejected before writing excess bytes. Previously staged files plus
  the current snapshot never exceed the admitted payload-byte budget. Filesystem
  allocation/metadata overhead is not included; no free-space reservation is
  promised. ENOSPC/permissions/allocation errors remain visible and release
  partial snapshots.
- Preparation has a fresh operation budget using the bound reviewed limits,
  shared across destination HEAD/hash checks and all source staging. The original
  non-sliding run lease still caps the entire lifecycle. Deadline/cancellation
  covers requests, streamed bodies and checks between cooperative disk calls.
- Only the first 50 exact mappings are included in the report, visibly marked
  when truncated. This report is read-only and cannot execute hidden mappings;
  a future execution review needs its own complete native selection review.
- Counters show actual local/S3 bytes read and logical SDK calls for this stage,
  not a misleading zero-network preview. Errors use existing initially collapsed,
  copyable diagnostics; SDK failures preserve safe provider/HTTP meaning without
  exposing payloads or raw secret-bearing source chains.

Tests cover native ownership/cross-manifest/selection rejection, source/destination
drift, opaque validators versus real hashes, symlink substitution, descriptor and
disk admission, private unlinked snapshot contents, selection policies/skips,
cancel/expiry/generation/client invalidation, no payload mutations, UI late
responses and separate approval. The disposable loopback MinIO test now also
prepares replacement snapshots and verifies both original payloads unchanged.

## Next gate

The read-only [journal foundation](job-journal.md) implements local attempt
durability, reconciliation and review pauses. Still required before exposing
execution: the executable-intent/outcome protocol and separately reviewed derivation of a guarded
writer from native plan ownership, attribute/encryption preservation, explicit
single-use execution approval, verified conditional writes, partial/uncertain
outcomes and failure-injection/provider coverage. Scheduling remains separate.
