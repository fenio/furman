# S3 navigation cache and activity inspector

## Display-only navigation cache

Returning to a previously completed prefix can reuse its in-memory listing.
The cache is scoped to the live connection ID, bucket and exact prefix. Separate
connections to the same bucket never share cached rows; reconnect/disconnect
invalidates the cache, including reused IDs and changed profile credentials,
endpoints, proxy settings or SSE-C keys. No secret values are part of cache keys.

Admission requires an exhaustive, fully settled progressive listing. Failed,
cancelled, partial, disposed and mutation-overlapping listings are not admitted.
Results and returned rows are copied; sorting/filtering/selection remain local
panel state. The limits are:

* **32 views**, LRU eviction;
* **50,000 total retained rows**, including parent entries;
* **16 MiB estimated storage** (UTF-16 strings plus conservative per-row overhead,
  not a hard measurement of the JavaScript engine's heap);
* **60 seconds since collection**, not extended by cache hits.

Oversized listings still browse normally but are not cached. The cache does not
cap active-panel memory. It does not persist to disk, cache object payloads,
cache bucket discovery, or implement globally sorted scans. Expired entries are
pruned on the next cache access/statistics read; already-open views remain visible
and show a freshness warning rather than silently changing selection.

The pane status sits below the file view and above the selection/free-space footer,
outside the scrolling rows in list, icon and column views. Listing statistics,
Refresh/Cancel/Retry/Continue controls, freshness notices and incomplete/error
diagnostics stay there rather than above the first row. Routine counts, LIST/page
statistics, timing, cache-age/fetch explanations and Activity/cache access are
hidden by default behind **Diagnostics**. Each pane has an independent disclosure
button with expanded/collapsed state; opening or closing it does not refresh,
scan, change selection, alter the cache or persist a preference. Refresh, compact
loading/incomplete/stale status, partial-selection warnings and errors remain
visible without expanding it; long error details remain separately collapsed and
copyable. A healthy complete listing has no routine status text outside Diagnostics.
It distinguishes cache hits from network listings, displays age,
and shows invalidation/expiry. Cached visits make no S3 LIST requests. Same-path
reloads, including the pane Refresh button and keyboard refresh, always invalidate
and fetch from the network. Cached complete results are still point-in-time
observations, not snapshot isolation or a watch on external changes.

All mutating calls routed through the frontend S3 service invalidate the entire
navigation cache **before and after** invocation, even on failure/cancellation.
This deliberately trades some hit rate for simple cross-pane/cross-connection
safety. Concurrent mutation lifetimes block cache reads/admission until all have
settled. Revision tokens prevent an older pending listing from repopulating the
cache after invalidation. Unknown service commands default to invalidation;
recognized reads and local settings/inspector commands do not.

Native recursive scans, comparisons, guarded previews and destructive-operation
preflights never consume this cache. Existing operations still enforce their own
safety contracts; cached display rows are not revision-guarded delete approvals.
Changes made by another application, an external MCP process, another device or
bucket lifecycle rules are only discovered by a fresh listing/operation. New
frontend write entry points must use the S3 service wrapper or explicitly
invalidate the display cache.

## Activity inspector

Open **Activity / cache** in an S3 pane, **Preferences → Network → Open inspector**,
or **S3 activity and navigation cache** in the command palette.

App-built S3 SDK clients collect a bounded, in-memory ring of the most recent
**500 completed logical SDK calls**. Each record contains only:

* monotonic sequence ID and start timestamp;
* SDK operation name, elapsed milliseconds, outcome and HTTP status;
* bounded, format-checked `x-amz-request-id`, when available;
* transmit attempts and retries within that logical call.

Outcomes are success, failed, timeout or abandoned. Dropping an unfinished SDK
future records an abandoned call; it may have reached the server. No request is
replayed or modified for observation. Original retry behavior and connection
interceptors, including SSE-C and guarded writes' disabled retries, are retained.
Clearing activity advances a generation so older in-flight calls do not restore
cleared records. Eviction counts are since the last clear; summary statistics
cover retained calls only. The table displays at most 100 newest matching rows;
the export includes all retained rows independently of the display filter.

Timing ends when the SDK returns its response/error, **not** when a GET body is
fully consumed. HTTP success can therefore precede a later download-body failure.
Transmit-hook counts are not billing totals or exact server receipts. Requests
abandoned before transmission can have zero attempts. Failures before SDK
operation metadata becomes available may use the name `S3Request`. Credential
provider, STS, KMS, S3 Control and CloudFront calls are outside this S3 inspector.

The inspector refreshes every two seconds only while open and Live is enabled;
refresh errors are visible and pause live refresh. Clearing the navigation cache
does not clear activity, or vice versa.

### Export privacy

**Copy diagnostics** copies an explicitly allowlisted, versioned JSON snapshot.
It does not export application logs, connection/profile settings, access keys,
session tokens, proxy credentials, SSE-C keys/fingerprints, request URLs, query
strings, bucket/object names, request/response bodies, raw provider error messages
or SDK Debug output. Observation never reads request inputs or request headers;
only HTTP status and the allowlisted request-ID response header are inspected.
Unexpected/oversized provider IDs are omitted. Timestamps and provider-controlled
request IDs remain in the export: review them before sharing. Useful full error
messages remain available in the existing operation-specific ErrorNotice, not
silently copied into the activity export.

Activity is session-only and bounded; there is no telemetry upload or automatic
file export. OS clipboard persistence and existing application-log retention are
separate from this inspector's in-memory history.
