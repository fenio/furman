# All-backend comparison and transfer plan

The target is a single Compare → reviewed one-way copy/update workflow across
local folders, S3, SFTP and archive sources. This is staged in separate PRs; enabling
a backend in Compare does not enable mutation commands or grant transfer authority.
No releases, schedules, automatic retries/resume, deletion/mirroring or new archive
rewriting are part of this work.

## Stage 1: read-only comparison (this PR)

All 16 pairs of **local, S3, SFTP and archive** roots can be compared in the pane
Compare dialog. Size/date is explicitly metadata-only; SHA-256 reads all included
contents and defaults on whenever either pane is not local. Paths, connection IDs,
method and exclusions are captured for retry/content verification independently
of later pane navigation. Archive root/nested URI boundaries retain correct pane
colours and filters. The existing built-in text diff remains local/S3 only.

SFTP comparison:

- Uses the original live connection ID and validates the URI's exact host/port
  against it. Opens a comparison-owned SFTP subsystem on the same authenticated
  SSH connection. No reconnect, credential reads, remote shell/checksum commands
  or temporary payload files. SSH authentication/host-key policy is unchanged.
- Walks directories iteratively with a 128-level depth limit; rejects duplicate,
  invalid and traversing names. Exclusions prune subdirectories and files before
  contents are read. Encountered symlinks/special entries fail rather than silently
  becoming regular-file comparisons; users can explicitly exclude them.
- Hashes read-only handles in chunks and checks size/type/permissions/mtime before
  and after against both the handle and path. Missing attributes, read failures,
  unexpected lengths and detected drift fail the comparison without a metadata
  fallback. This is not a filesystem snapshot: same-size/same-time edits and remote
  symlink/ancestor races cannot be ruled out by SFTP v3 checks. Keep roots stable.
- Shares the scan entry/deadline/cancellation limits with the other side. Raw SFTP
  directory pages and sequential 32 KiB reads are individually budgeted, including
  EOF, attribute checks, opens/closes and subsystem setup. SSH transport packets
  are not counted as SFTP requests. A received protocol packet is capped by the
  library at 256 KiB, accumulated metadata at 32 MiB estimated, and directory depth
  at 128. Empty/repeated/invalid pages cannot silently establish completeness.
  Cancellation/error drops the comparison-only subsystem, closing outstanding
  handles without closing the pane's SFTP session. It never buffers a whole
  directory, recursive listing or complete file payload before checking limits.

Archive comparison:

- Accepts the existing `archive://<absolute-local-archive>#<internal-folder>` roots,
  including temporary archives already opened through a remote pane. It compares
  that local archive copy, not a fresh remote version. A deleted/replaced temporary
  archive must fail rather than reconnect or redownload automatically.
- Requires 7zip. Listing is a cancellable, deadline-bound subprocess with stdout
  capped at 16 MiB and stderr at 8 KiB; stdin is closed and child processes are
  killed on dropped futures. Member hashes stream expanded contents with `-so`
  and `-spd` (literal names, no wildcard expansion). Nothing is extracted to disk.
- Strict UTF-8/member metadata parsing refuses duplicate/ambiguous paths, traversal,
  special/link entries, file/directory collisions, encrypted members and unsupported
  names. Unlike the ordinary archive browser, it never normalizes backslashes or
  accepts lossy path parsing as complete. Conservative refusal applies even to
  unsafe members outside the chosen prefix/exclusions.
- Reads exactly each member's listed size, checks subprocess success, and rechecks
  the archive's metadata/identity after comparison. A 7zip warning/error, missing
  selected internal folder, corruption or changed archive cannot produce success.
  Archive mtimes without a stored value display as zero; cross-format dates are not
  proof of content equality, so SHA-256 is recommended.

Both collectors participate in the existing terminal ScanStopped/Done protocol;
partial/failed/cancelled/limited results never establish equality or authorize a
transfer. Native S3 comparisons also reject a URI bucket that differs from the
selected connection instead of reading another bucket through those credentials;
literal trailing prefix slashes and percent sequences are preserved.

**Stage boundary:** Sync and saved jobs retain their existing backend gates. The
Compare dialog explains that SFTP/archive combinations are read-only; the Sync
handoff and text-diff action are disabled for those pairs. Existing local/S3
handoffs still require a fresh comparison and guarded S3 write review. There is no
legacy-copy fallback from an unsupported comparison pair.

## Next stages (not implemented in this PR)

1. **Exact relative mappings and local destinations:** introduce a native reviewed
   mapping for every selected file, including nested paths; reject traversal,
   aliases/overlapping roots, file/directory conflicts and names that cannot be
   represented at the destination. Retire the legacy flattening path rather than
   lifting the guide's nested-local refusal over the old engine.
2. **SFTP copy/update:** support local↔SFTP, SFTP↔SFTP and SFTP↔S3 with cancellable
   streamed/private staging, checked sources/destinations, visible partial/uncertain
   failures and mandatory content verification. Retain connection identity and
   source/destination direction through review. SFTP does not provide S3-style
   conditional replacement: destination race/rename semantics must be reviewed
   explicitly, not advertised as equivalent to the native saved-job guarantees.
   No unguarded fallback may weaken the existing conditional S3 writer.
3. **Archive source mappings:** extract only reviewed regular members into bounded
   private staging, preserve relative destinations and archive identity, then use
   the destination's reviewed writer. Archives remain read-only destinations;
   archive rewriting would require a separate explicit design/authorization.
4. **Guide and reusable workflows:** expose each writable pair only when its native
   mapped engine and tests are present. Existing saved-job pins, keychain/SSE-C,
   mandatory SHA-256, single-use approvals, journals, recovery/cleanup and supported
   limits remain unchanged; new backend job authority is a separate extension,
   never inferred from ordinary connection IDs or read-only results.

## Validation boundaries

SFTP unit tests run the actual russh-sftp protocol over private in-memory duplex
streams, asserting read-only open flags, recursion/exclusions, content hashes,
authority mismatch, malformed/duplicate names, symlinks, permission failures,
short/long streams, metadata drift, request/entry limits, stalled cancellation and
deadlines. They do not modify SSH trust, use real keys or contact remote servers.
A separate explicitly configured loopback OpenSSH/SFTP fixture verifies the actual
second-subsystem setup, nested hashes/exclusions, unchanged payloads and continued
operation of the original pane session. Local runs isolate fixture host trust in
a temporary HOME; CI enrolls only its newly created disposable server's key.
Archive parser/pipe-limit tests run without 7zip; an explicitly invoked disposable
7zip fixture checks ZIP/7z/TAR literal wildcard names, hashes, exclusions, limits,
cancellation, missing prefixes and unchanged archive bytes. CI runs it on macOS
and Linux with 7zip installed. UI tests cover all backend pairs, immutable remote
IDs/options through retry, nested archive colours and refusal of write/diff handoffs.

Live production SSH/SFTP, arbitrary archive formats and cross-provider transfers
are not established by these fixtures. No production datasets or credentials are
used by this validation.
