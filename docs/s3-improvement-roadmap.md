# S3 improvement roadmap

Agreed follow-up order from the Brows3 review. Statuses distinguish merged,
current, and future work. Keep each substantial change in a
separate PR, with provider-specific behavior and safety tested explicitly.

1. **Progressive listings — merged in [#355](https://github.com/fenio/furman/pull/355).** Page-based S3 browsing, immediate first
   results, partial/complete status, Cancel/Continue/Retry, stale-navigation
   protection, and explicit loaded-only sorting/filtering/selection semantics.
2. **Scan safeguards — merged in [#356](https://github.com/fenio/furman/pull/356).** Configurable object/request/time budgets for recursive
   search and enumeration; scanned/matched counts; complete/cancelled/limited
   outcomes; no destructive work from incomplete preflight. Show bytes read and
   potential transfer costs for SHA-256 comparison. Harden all cursor handling.
   See [the implemented contract](s3-scan-safeguards.md).
3. **Guarded writes — current PR.** Create-only and replace-if-unchanged policies for PUT and
   multipart completion, approved local snapshots, explicit overwrite approval,
   and fail-closed behavior on providers without the needed conditional writes.
   See [the guarded content-write contract](s3-guarded-writes.md) for supported
   entry points, costs, and explicit exclusions.
4. **Bounded navigation cache and diagnostics.** Per-profile/revision cache with
   invalidation, visible freshness, manual refresh, and no stale destructive
   preflight. Add an activity inspector with operation, duration, status, request
   IDs, retries, counts, and a safe diagnostic export.
5. **Metadata and provider compatibility.** Attribute-preserving metadata edits
   above 5 GiB, MIME inference, conflict guards, tested checksum policies, and
   single-delete fallback only for recognized unsupported bulk-delete errors.
6. **AWS SSO usability.** Profile discovery, clear expired-session states, and an
   explicit AWS CLI v2 sign-in action without storing SSO tokens ourselves.
7. **Saved jobs, then opt-in scheduling.** Start with reusable comparison/sync
   presets. Later add pinned destinations, fresh comparisons, persistent outcomes,
   failure pauses, and explicit approval; never silently schedule uploads.

Retain Svelte, the dual-pane workflow, SHA-256 verification, SSE-C support, and
keychain-backed secrets. Do not copy Brows3's plaintext secret-storage fallback
or assume its documented next-release features are already released.

## Progressive-listing contract

- Browsing uses one delimiter-based LIST page at a time (up to 1,000 S3 results).
  Existing complete-list APIs remain available for MCP and other callers; sync,
  copy and delete continue to enumerate their own full scope in the backend.
- Results appear after each page. Sorting and filtering cover loaded entries;
  the UI never claims global ordering/completeness until the final page arrives.
  Select all selects currently loaded matches only; later pages are not silently
  added to the selection. Selected folders still represent their recursive scope
  when used in normal copy/delete operations, not merely visible child rows.
- A failed page leaves accepted results visible. Retry requests that same page;
  Refresh starts a fresh listing. This is not snapshot isolation: a bucket that
  changes during pagination can yield a changing view.
- Cancel stops further paging and discards an in-flight result. It does not
  remotely cancel an HTTP request already sent. Continue waits for that request
  to settle. Navigation/closing a tab detaches old results immediately.
- LIST calls count app-level page requests, including retries initiated by the
  user, not individual SDK wire attempts. Pages count accepted successful pages.
- Progressive pane browsing itself has no cache, automatic retry loop, or
  recursive-scan cap. Loaded entries still consume memory proportional to the
  listed prefix. The separate [scan budgets](s3-scan-safeguards.md) apply to
  recursive work and complete-list APIs; bounded caching remains future work.
