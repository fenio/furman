# Durable attempt journal and review pauses

The foundation merged in [#366](https://github.com/fenio/furman/pull/366) extends
[checked native previews](job-checked-previews.md) with native attempt intents,
stages/outcomes, abandoned-attempt reconciliation and explicit review pauses.
Its v1 schema was read-only and could not represent payload intent. The
[initial guarded-execution follow-up](job-guarded-execution.md) now introduces
v2, retaining v1 read compatibility and adding executable intent/outcome counters
for separately approved local → S3 creates up to 5 MiB/file only. **No history or
recovery action approves execution. No retry/resume or scheduling is enabled.**

## Local-only startup and manual review

The GUI requests native local history/reconciliation on startup. It reads no
keychain values, loads no credential chains, connects to no endpoint and scans
no source. Native binding also checks/reconciles the journal before starting
new work, independently of frontend startup state.

A nonblocking native run lease must be acquired before reconciliation, review
or removal. If another cooperating process/task owns it, history is a read-only
snapshot marked **reconciliation deferred**. A live foreign attempt is never
declared interrupted merely because a second app window opened. Review/removal
remain disabled until the owner releases its lease and actual workers stop.

With the lease held, nonterminal records become `interrupted`, and their job is
paused for review. Recovery is idempotent: a second successful check does not
rewrite history or change its revision. It restores no SDK clients, root/file
descriptors, manifests, payload snapshots or approvals. Restart always requires
a new explicit binding/comparison/preparation sequence.

Open **Sync → Saved job definitions → Local attempt history** to inspect safe
records, refresh local recovery, acknowledge a selected job pause or acknowledge
a separate storage-recovery pause. Acknowledgements are explicit and bound to
the last native history revision. They do not retry, reconnect, restore approval
or execute anything. Job review and storage-health review are separate gates;
clearing one does not silently clear the other. History failures and unresolved
pauses remain visible in a global notice even if the originating dialog closes.
Long diagnostics use initially collapsed, copyable notices.
Late startup/refresh replies cannot clear a newer recording failure or overwrite
a newer review report; these shared notices remain visible after dialog close.

## Native lifecycle and ownership

After native profile/root validation, binding durably records a `binding` intent
**before** reading the first keychain entry or sending a bucket request. The
intent is tied to the native run ID, definition ID and exact revision. A review
pause or unavailable journal blocks this work before credential/network access.

The supported read-only state sequence is:

```text
binding → bound → scanning → compared → preparing → prepared
                     ↘ no_changes
```

Terminal outcomes are `no_changes`, `discarded`, `cancelled`, `failed`, `limited`
and `interrupted`. `no_changes` means a complete comparison found no actionable
copy/update entries; destination-only entries remain informational and it is
not proof of namespace identity. `prepared` is **not** executable approval or
transfer success. Releasing compared/prepared work discards it; cancellation,
expiry, failures and abandoned tasks do not become successful transfers.

Each stage change is committed before the next read/staging stage can start.
Preparation records the selected allowlisted policy before destination/source
reads, never an approval token. Native worker/attempt `Arc` leases retain the
run lock through blocking work and outcome recording. Dropped tasks become
interrupted conservatively; process crashes leave the last durable active intent
for startup recovery. Mutexes are never held across network awaits.

Failures, budget-limited attempts, interruptions and partial/uncertain execution
outcomes pause the job. Journal
recording failures invalidate pending native manifests/previews, cancel further
work and latch a storage-review requirement. An error after a journal write may
mean its update already committed; no payload/read stage is replayed to repair
it. Corrupt/unreadable data is preserved rather than replaced by empty history.
Ordinary Copy/Sync, saved configuration and connection workflows remain separate.

Attempt records contain only canonical random job/run UUIDs, numeric definition
revisions, timestamps, allowlisted stage/outcome/policy values and per-stage
enumeration/request/content-byte/elapsed counters. They contain **no names,
paths, endpoints, client-instance IDs, hashes/ETags, credentials, encryption keys,
payloads, raw provider messages or approval tokens**. Counters are diagnostic
stage-boundary observations, not a billing ledger or proof in-flight requests
were recalled. Known S3/SSO/provider failures retain their useful live diagnostics
without copying those messages into persistent history.

## Storage and durability protocol

Storage is separate from definitions and preferences:

```text
~/.config/furman/job-runs/             owner-only directory
  journal.json                      private bounded strict v2 schema (v1 readable)
  initialized                       fixed, non-secret initialization evidence
  dirty                             fixed recovery evidence during commit
  lock                              cooperating-process metadata lock
```

New directory/file permissions are `0700`/`0600`. Reads verify ownership,
private permissions, regular-file type, one link, no-follow opens and bounded
size. Symlinks, hard links, FIFOs/special nodes, corrupt/future/unknown schemas,
invalid marker contents and oversized data fail closed. Directory identity is
checked across commits. A previously initialized journal going missing is an
error—not an empty-store migration that could hide an unresolved intent.

Each serialized update follows this order:

1. Validate the strict schema/limits and allocate a fresh revision.
2. Write/sync a conservative `dirty` marker and sync its directory entry.
3. Write a private same-directory temporary JSON file; sync its contents.
4. Atomically replace `journal.json`; sync the directory.
5. Ensure/sync fixed initialization evidence and its directory entry.
6. Remove the dirty marker and sync the directory again before returning success.

On macOS, regular-file barriers additionally require `F_FULLFSYNC`; there is no
silent fallback to weaker `fsync` behavior. Linux uses file and directory
`fsync`/`sync_all` barriers. Unsupported flush/permission/filesystem errors are
visible, include safe OS error categories where available and block job work.
Native history/review/status/release IPC uses blocking workers so slow local
filesystem calls do not run on the GUI thread. Run locks remain nonblocking;
the separate metadata lock has a bounded two-second wait, allowing short history
reads and stage commits to serialize without spuriously cancelling each other.
Lock guards explicitly unlock before closing to handle fork-inherited duplicates.

If a commit is ambiguous, recovery first durably records a storage-review pause
before clearing dirty evidence. The process also retains an in-memory failure
latch, including failures before a first intent could be written. Unwritable
storage cannot guarantee persistence of a *new* error; the last durable active
intent and marker provide the restart backstop. If no intent could be committed,
no keychain/network work started. A failure after a durable terminal update can
leave that accurate outcome present even though final marker cleanup/reporting
failed; it is never converted into automatic continuation.

These barriers rely on a supported local filesystem and storage honoring flushes
and atomic same-directory replacement (for example normal local APFS/ext4-style
deployments). This is not a claim about arbitrary NFS/SMB/FUSE/cloud-synced paths,
faulty controllers, external editors ignoring locks, malicious same-user code or
physical-media integrity. Tests inject failures at protocol commit points; they
are not hardware power-cut certification. Private metadata is plaintext, not an
encrypted vault. A crash can leave an orphan private temporary **metadata** file;
recovery does not broadly delete unknown files. Payload snapshots remain private
unlinked capabilities and are never persisted/restored by this journal.

## Retention, deletion and safety gates

- At most 500 retained terminal attempts globally and 50 per job; the native read
  cap is 8 MiB. Only oldest resolved terminal records can be evicted. Active
  intents and records referenced by unresolved review pauses are never evicted
  to make a new attempt possible. Insufficient safe space refuses new work.
- Definition deletion is refused while any cooperating live job task holds the
  single global run lease. Deleting a definition never deletes its history or
  hides its terminal review pause.
- History removal requires its own confirmation/revision, the exclusive lease,
  no active intents and no unresolved job/storage pauses. It removes resolved
  terminal history only, not definitions or credential data.
- Native malformed/stale acknowledgements, unknown IDs and duplicate/terminal
  transitions cannot grant authority. History IDs/policies are display records,
  never a substitute for fresh native comparison/snapshot checks.

Tests cover strict/privacy-safe roundtrips, every durability checkpoint,
corrupt/future/oversized/linked/special storage, missing initialization evidence,
ownership/lock contention, retention, idempotent/exclusive recovery, revision-bound
review, pre-keychain failures, read-only stage/discard recording failures,
manifest/preview invalidation, surviving definition deletion, global notices,
late replies and the absence of connection/execution calls during review.

## Initial execution and remaining gates

[Initial guarded execution](job-guarded-execution.md) adds expiring single-use
native approval, snapshot consumption, pre-mutation durable intent and mandatory
SHA-256 verification with confirmed/partial/uncertain outcomes. The read-only
sequence described above remains preparation only; see that companion for the
executable v2 lifecycle, counter protocol and fault behavior. S3-source attribute/
encryption/ACL/tag preservation, replacements and multipart job execution need
separate review/testing. Scheduling remains a separately authorized later proposal.
