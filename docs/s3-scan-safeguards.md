# Scan safeguards

Configure **Preferences → Network → Scan limits**. Defaults for new scans:

| Budget | Default | Supported range |
| --- | ---: | ---: |
| Entries scanned | 100,000 | 1–10,000,000 |
| S3 API calls | 1,000 | 1–1,000,000 |
| Absolute deadline | 300 seconds | 1–86,400 seconds |
| S3 search matches | 1,000 | 1–10,000 |

Zero does not disable a budget. Settings are persisted and validated by both the
UI and Rust backend. A scan captures its limits when it starts; later settings
changes do not change a running scan. The standalone MCP process uses defaults.

## Scope and counting

- S3 recursive search, comparison/sync preparation (including local files), and
  S3 copy/download/delete/folder-rename enumeration use operation-scoped budgets.
  A comparison shares one budget across both sides; multi-prefix preflight shares
  one budget across the whole selection, rather than resetting for each folder.
- Entries count what is visited, including excluded files, directory markers,
  and repeated/overlapping selections. Local comparison also counts directories.
  Filtering does not bypass scan limits. Local filename/content search retains
  its existing independent behavior; it is not covered by these settings.
- API calls count app-level LIST/GET/HEAD invocations during scanning, not SDK
  retries, wire requests, or the subsequent mutation/payload-transfer phase.
- The deadline covers the entire scan, not a fresh timeout per page/object. It
  also interrupts waiting for asynchronous S3 requests or stalled hash streams.
  Cancellation is checked during those waits and between local reads. Local OS
  filesystem calls are cooperative and cannot be forcibly interrupted mid-syscall.
- Complete-list APIs (including MCP), version-history listings, incomplete-upload
  listings, and inventory pagination also have budgets and cursor checks. Normal
  progressive pane browsing remains separate: it can be cancelled/continued and
  is not silently truncated by a recursive-scan budget.

## Outcomes and safety

Search/comparison show **complete**, **cancelled**, **limited**, or **failed**.
Scanned/matched counts, API calls, and elapsed time remain visible. A cancelled,
limited, or failed scan does not claim “no matches” or content equality.
Search retains partial matches but labels them incomplete. Reaching the search
match cap stops scanning; only a provably final object on the final page can
finish exactly at the cap and still count as complete.

Sync requires an explicit successful completion event. Cancelling, hitting a
budget, changing comparison options, or failing clears approval/selection and
disables Sync until a fresh exhaustive comparison succeeds.

Recursive enumeration returns an error, never a partial successful vector. S3
delete, copy, download, and folder rename finish their whole enumeration before
starting their mutation/payload phase. A failed/cancelled/limited preflight
therefore does not start that phase. Missing, empty, repeated, or cyclic cursors
fail rather than silently truncating or restarting a listing. Paired version and
upload markers are checked as pairs so advancing versions of one key stay valid.

These protections are not snapshot isolation, guarded destination writes, or
rollback. Failures after the mutation phase starts can leave completed work;
existing transfer policies still apply. Guarded writes are the next roadmap PR.

## SHA-256 visibility and charges

Comparison reports content bytes read and S3 payload bytes consumed for hashing,
including bytes consumed before an eventual failure. These are **not billing
totals**: SDK retries, buffering/read-ahead, discarded in-flight responses, and
protocol overhead are not measured. They are not a dollar-cost estimate.

SHA-256 reads included content from both sides. S3 request and download/egress
charges can apply, even when a comparison fails or is cancelled. There is no
separate byte budget in this change; choose an appropriate prefix/deadline before
scanning large datasets. Excluded contents are not hashed, but still count toward
enumeration limits. Local comparison rejects non-regular files (such as FIFOs)
instead of blocking on them; symbolic-link directories remain skipped.
