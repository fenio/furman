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

**Folder selection:** “Browse folders…” opens a read-only in-app folder browser
for local directories or the selected connected S3 bucket. Open a subfolder or
go to its parent, then explicitly choose **Use this folder**. Pane paths are
labelled shortcuts, not the only available folders. There is no misleading
one-option dropdown when only local storage is available; the guide explicitly
explains how to make an S3 connection available. Storage-type buttons do not
list folders or connect automatically, and clicking an already selected type
does not reset a custom path.

Only explicitly opening the folder browser/navigation requests metadata. S3
pages load on request (at most 20 per folder; 1,000 visible folders), retaining
accepted folders on failure and retrying the same page without accepting cyclic
tokens. Local browsing uses the existing non-recursive directory metadata API;
it can enumerate the directory before applying the visible-folder cap. Symbolic-
link folders are not offered. Closing/changing location discards late replies;
in-flight OS/network reads may finish cooperatively. Typed-but-unopened browser
paths cannot be mistaken for its confirmed selection. S3 bucket/connection IDs
stay pinned and literal prefixes are never URL-decoded. Browse results select
only a location: they never authorize a comparison, saved job or transfer, and
the fresh complete-comparison/approval requirements below remain unchanged.

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

**Advanced comparison options and limits** contains editable maximum entries,
S3 API requests and duration (seconds), initialized from Preferences. Overrides
apply only to this comparison across both roots; they are validated in the UI and
native command and never change global defaults. Reset explicitly reloads the
Preferences values. Editing or resetting invalidates results/selections and cancels
the current scan without starting another; a fresh complete comparison is required.
Saved method/exclusion presets are a separate section, not a substitute for editing
these fields, and do not store scan-limit overrides. Saved native-job budgets are
unchanged by these one-off comparison controls.

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
