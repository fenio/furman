# Guided Sync and Compare handoff

⌘Y / **Sync** opens a three-step setup rather than the old flat pane comparison
plus unrelated job editor. This is a UI follow-up to [guarded multipart jobs](job-guarded-multipart.md),
not a new write cohort, scheduling feature or release.

1. **Source:** confirm the active pane's original folder, type a different absolute
   local directory or choose a connected S3 location. Selecting/typing never connects,
   scans, prepares or transfers. An existing saved transfer can be selected here;
   it opens its saved revision in the final review step without any network work.
2. **Destination:** confirm the opposite pane's folder, or choose/edit another
   supported location. Same/overlapping local roots and same-connection S3 prefixes
   are rejected early. Literal S3 prefixes are not URL-decoded/rewritten, must end
   in `/`, and cannot change the connected bucket. Native identity/origin checks
   remain authoritative for aliases; text checks do not prove filesystem/server identity.
3. **Review & transfer:** choose “Add missing files only” or “Add missing files and
   update changed files”, explicit exclusions and a comparison method. Click
   **Compare**, inspect/select the differences, then explicitly copy/review writes.
   Destination-only entries are informational and **always kept** in this flow;
   neither Select All, row clicks nor keyboard selection can authorize deletion.
   Enter outside an explicit button no longer launches a transfer.

**Local-destination limitation:** the existing one-off copy/download engine uses
basenames for individually selected nested files rather than carrying each exact
relative destination mapping. The new guide refuses such nested selections before
dispatching *any* files; it never silently flattens them. Direct files at the compared
root remain available. S3-destination guarded transfers preserve their exact mappings.
A local-destination engine update is a separate follow-up, not silently bundled
into this presentation change. Ordinary folder Copy remains separate.

The guide captures source/destination connection IDs and paths on opening; pane
navigation cannot silently retarget it. Editing setup/back navigation unmounts the
old comparison/session, cancels/releases its work and discards approvals/results.
Final dispatch carries the reviewed edited roots, not stale modal defaults or
values cleared by closing the modal. Incomplete/failed/cancelled/limited comparisons
cannot enable writes. Failures remain visible with initially collapsed, copyable
diagnostics; the old duplicate raw error in the footer is replaced with a short
reference to the diagnostic.

## Optional reusable jobs

Destination setup offers **Save this setup for next time (optional)**. One-off
transfers continue through existing connection/transfer engines; saving is not
required. Eligible connected saved access-key S3 profiles can instead be saved
with a name, exact folder prefixes and explicitly reviewed exclusions. No secrets
are copied from live connections: profile IDs are hints, native profile resolution,
configuration-revision checks and generation pins remain authoritative. Saved
profile settings/services, not unsaved live credentials, are disclosed as the
authority for the reusable job. Saving only persists configuration and inspects
local directory metadata; it neither reads the keychain nor connects/scans/writes.

The saved job then uses the existing native workflow with friendlier labels and
technical details collapsed: **Connect for comparison (read-only)** → **Compare
files** → explicitly approved **Prepare selected files (no uploads)** → **Review
transfer** → separate single-use execution/exclusion/replacement/multipart approvals.
All native caps, supported-attribute gates, conditional writers, journal barriers,
snapshot privacy, verification and uncertain-outcome rules are unchanged. Saved
jobs still require SHA-256 and support local/S3 → S3 up to 1 GiB/file, not dynamic
AWS-auth execution or local destinations. One-off transfers do not inherit native
job history/approval guarantees merely by using the same guide.

Unconnected saved profiles must first be connected in a pane for the one-off
location picker. Existing saved jobs require no ordinary connection. Advanced
creation/rebinding/deletion of definitions and local history remain under
**Manage saved transfers and local history** in step 1. Selecting a stale job
requires explicit advanced review/rebind rather than silently accepting changed
settings. History/recovery remains local-only. No schedule, retry, resume, deletion
or unguarded overwrite is implicitly authorized.

## Compare → Sync

After a complete successful pane comparison, each comparison bar offers **Sync
from this side…** when that direction has new/modified files. The right-side action
reverses the *original compared roots/IDs*, not current pane navigation or labels.
The guide receives copies of the comparison method and exact exclusions. Stopped,
failed, cancelled, partial or limited comparisons cannot expose an executable handoff.

This is an **intent-only setup handoff**, not reuse of executable results: no rows,
hashes, cached listings, native manifests, snapshots or approvals are imported.
The guide explicitly requires another fresh comparison before reviewed writes.
Opening the handoff stops the old pane comparison to avoid presenting stale
colours/counts as transfer authority. No connection or transfer is started by the
handoff or by applying an options preset; switching to a saved transfer uses that
job's explicitly displayed saved roots/options instead.

Controller/SSR tests cover all three setup steps, no automatic actions, optional
configuration-only saves, late/failed saves, exact edited-root dispatch, both
comparison directions, incomplete-result refusal, copy-only policies, keyboard
protection and unchanged native approval gates. Validation uses mocks/disposable
fixtures, not production buckets, credentials or real keychain entries.
