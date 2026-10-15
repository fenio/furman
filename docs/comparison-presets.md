# Saved comparison / sync options

Compare panes and Sync share named presets for **comparison method** (SHA-256
or size/date) and **explicit exclusion patterns**. Open “Saved comparison / sync
presets” in either dialog. Choose a saved preset and click **Apply selected
options**. Merely selecting a preset does not apply it. Review the visible
options and current roots before starting a comparison.

## Scope and safety

- Presets are reusable options, not runnable jobs. They contain no paths,
  connection identities, endpoints, credentials, encryption keys, direction,
  selections, deletion policies, write policies, results or approvals.
- Applying a preset does not navigate panes, connect, change the source or
  destination, alter global scan budgets, run a comparison or execute a transfer.
  SHA-256 remains the default for comparisons involving S3; opting into a
  metadata preset is explicit and labeled as not verifying contents.
- Empty exclusions remain empty. No patterns are silently added by presets.
  Existing Sync default exclusions remain visible until explicitly changed.
- Sync no longer starts scanning when the dialog opens. Review source →
  destination, method, exclusions, scan budgets and potential hashing costs,
  then click **Compare**. This allows choosing a preset before network work.
- Applying a preset in Sync invalidates the current scan and selection, requests
  cancellation, and discards late events. **Rescan** must complete successfully
  before Sync is enabled. Stopped, failed and limited scans cannot authorize it.
- Sync still passes through its existing execution flow, including native fresh
  destructive preflights and single-use guarded destination-write previews where
  supported. No preset can approve overwrite, weaken Garage gates or bypass SSE-C.
- Saving or updating a preset saves the **currently displayed options**, not the
  old saved preview. Updates retain the preset ID and can rename it. Deletion
  requires a separate confirmation and does not change current dialog options.

## Persistence and diagnostics

- Native commands store a versioned document at
  `~/.config/furman/comparison-presets.json`, separate from general configuration
  so unrelated preference saves cannot overwrite presets. New files use Unix
  mode 0600. Names and patterns are ordinary user-entered configuration, not an
  encrypted secret store: do not put secrets into them.
- Saves use a same-directory temporary file, flush its contents, then atomically
  replace the document. An advisory sibling lock serializes cooperating Furman
  processes; lock contention fails without waiting. Revision checks reject stale
  updates rather than silently losing another window's changes. Refresh and
  review the list after a conflict. External editors that ignore the lock are
  not transactionally isolated; this is not a power-loss recovery system.
- Missing storage means an empty list. Malformed documents, unknown fields,
  unsupported versions, symlinks, nonregular files and unreadable storage fail
  visibly instead of silently resetting the list. Load failure disables changes
  until a successful refresh; save failure retains the last confirmed list.
  Repair/back up invalid storage externally; no automatic destructive reset is
  provided. Parser diagnostics do not reveal file contents.
- Bounds: 100 presets, 256 KiB total file size, unique case-insensitive names of
  1–80 characters, UUID IDs, and 128 exclusion patterns per preset of at most
  512 bytes each. Patterns follow the existing comma-separated dialog syntax;
  commas, control characters and surrounding whitespace are not stored.
- Errors use the existing copyable, initially collapsed detailed diagnostics.
  The collapsed preset section indicates errors in its summary. Connection and
  transfer failures remain owned by their existing dialogs.

## Explicitly deferred

Pinned source/destination identities, saved runnable jobs, automatic reconnect,
background execution, schedules, persistent run history, import/export and
stored scan budgets are not included. Those require a separate design and PR;
no scheduling timer or startup work is installed here.
