<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { syncDiff, cancelSync } from '#lib/services/tauri.js';
  import { appState } from '#lib/state/app.svelte.js';
  import type { SyncEntry, SyncEvent, PanelBackend } from '#lib/types/index.js';
  import type { ScanOutcome, ScanProgress } from '#lib/types/index.js';
  import { canUseScan } from '#lib/utils/scan.js';
  import { errorText } from '#lib/utils/error-report.js';
  import ScanStatus from './ScanStatus.svelte';
  import ComparisonPresets from './ComparisonPresets.svelte';
  import SavedSyncJobs from './SavedSyncJobs.svelte';
  import type { ComparisonMode } from '#lib/utils/comparison-options.js';
  import ErrorNotice from './ErrorNotice.svelte';
  import type { SyncPolicy } from '#lib/utils/sync-workflow.js';

  interface Props {
    sourceBackend: PanelBackend;
    sourcePath: string;
    sourceS3Id: string;
    destBackend: PanelBackend;
    destPath: string;
    destS3Id: string;
    onSync: (entries: SyncEntry[]) => void;
    onClose: () => void;
    embedded?: boolean;
    initialMode?: ComparisonMode;
    initialExclusions?: string[];
    initialPolicy?: SyncPolicy;
  }

  let {
    sourceBackend,
    sourcePath,
    sourceS3Id,
    destBackend,
    destPath,
    destS3Id,
    onSync,
    onClose,
    embedded = false,
    initialMode,
    initialExclusions,
    initialPolicy = 'update',
  }: Props = $props();

  let allEntries = $state<SyncEntry[]>([]);
  let scanning = $state(false);
  let scanComplete = $state(false);
  let scanError = $state('');
  let optionsSaveError = $state<unknown>(null);
  let jobPanelOpen = $state(false);
  let outcome = $state<ScanOutcome | null>(null);
  let scanProgress = $state<ScanProgress | null>(null);
  let pendingEntries: SyncEntry[] = [];
  let entryFlushTimer: ReturnType<typeof setTimeout> | undefined;

  function clearPendingEntries() {
    clearTimeout(entryFlushTimer);
    entryFlushTimer = undefined;
    pendingEntries = [];
  }

  function flushEntries() {
    clearTimeout(entryFlushTimer);
    entryFlushTimer = undefined;
    const batch = pendingEntries;
    pendingEntries = [];
    if (!batch.length) return;
    allEntries = [...allEntries, ...batch];
    for (const entry of batch) {
      if (entry.status === 'new' || (entry.status === 'modified' && policy === 'update')) selectedPaths.add(entry.relative_path);
    }
  }
  let filter = $state<'all' | 'new' | 'modified' | 'deleted'>('all');
  let selectedPaths = new SvelteSet<string>();
  let cursorIndex = $state(0);
  let currentSyncId = $state('');
  let listEl: HTMLDivElement | undefined = $state(undefined);
  let excludeInput: HTMLInputElement | undefined = $state(undefined);

  // Options
  let excludeText = $state(untrack(() => initialExclusions?.join(', ') ?? appState.syncExcludePatterns));
  let compareMode = $state<ComparisonMode>(untrack(() => initialMode ?? 'sha256'));
  let policy = $state<SyncPolicy>(untrack(() => initialPolicy));

  // Summary counts
  let newCount = $state(0);
  let modifiedCount = $state(0);
  let deletedCount = $state(0);

  let filtered = $derived.by(() => {
    if (filter === 'all') return allEntries.filter((e) => e.status !== 'same');
    return allEntries.filter((e) => e.status === filter);
  });

  onDestroy(() => {
    clearPendingEntries();
    if (currentSyncId) {
      cancelSync(currentSyncId).catch(() => {});
    }
    currentSyncId = '';
  });

  function parseExcludePatterns(): string[] {
    return excludeText
      .split(',')
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  function startScan() {
    const id = `sync-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

    // Cancel any existing scan
    if (currentSyncId) {
      cancelSync(currentSyncId).catch(() => {});
    }

    currentSyncId = id;
    clearPendingEntries();
    allEntries = [];
    selectedPaths.clear();
    cursorIndex = 0;
    scanning = true;
    scanComplete = false;
    scanError = '';
    outcome = null;
    scanProgress = null;
    newCount = 0;
    modifiedCount = 0;
    deletedCount = 0;

    // Persist exclude patterns
    if (appState.syncExcludePatterns !== excludeText) {
      appState.syncExcludePatterns = excludeText;
      optionsSaveError = null;
      appState.persistConfig().catch((error: unknown) => { optionsSaveError = error; });
    }

    const handleEvent = (event: SyncEvent) => {
      if (id !== currentSyncId) return;

      if (event.type === 'Entry') {
        pendingEntries.push(event as SyncEntry);
        if (!entryFlushTimer) entryFlushTimer = setTimeout(flushEntries, 50);
      } else if (event.type === 'ScanProgress') {
        scanProgress = event.progress;
      } else if (event.type === 'ScanStopped') {
        clearPendingEntries();
        scanProgress = event.progress;
        outcome = event.outcome;
        scanError = event.message;
        scanComplete = false;
        scanning = false;
        selectedPaths.clear();
        currentSyncId = '';
      } else if (event.type === 'Done') {
        flushEntries();
        newCount = event.new_count;
        modifiedCount = event.modified;
        deletedCount = event.deleted;
        scanning = false;
        scanComplete = true;
        outcome = 'complete';
      }
    };

    syncDiff(
      id,
      sourceBackend,
      sourcePath,
      sourceS3Id,
      destBackend,
      destPath,
      destS3Id,
      parseExcludePatterns(),
      compareMode,
      handleEvent,
    ).catch((err: unknown) => {
      if (id === currentSyncId) {
        clearPendingEntries();
        scanning = false;
        scanComplete = false;
        scanError = 'Comparison failed: ' + errorText(err);
        outcome = 'failed';
        selectedPaths.clear();
      }
    });
  }

  function invalidateScan() {
    const hadComparison = scanning || scanComplete || outcome !== null || allEntries.length > 0;
    clearPendingEntries();
    if (currentSyncId) cancelSync(currentSyncId).catch(() => {});
    currentSyncId = '';
    scanning = false;
    scanComplete = false;
    outcome = hadComparison ? 'cancelled' : null;
    selectedPaths.clear();
    scanError = hadComparison ? 'Options changed or scan cancelled. Compare again before transferring.' : '';
  }

  function applyPreset(mode: ComparisonMode, exclusions: string[]) {
    invalidateScan();
    compareMode = mode;
    excludeText = exclusions.join(', ');
  }

  function toggleSelection(path: string) {
    const entry = allEntries.find(entry => entry.relative_path === path);
    if (!entry || entry.status === 'deleted' || entry.status === 'same' || (entry.status === 'modified' && policy === 'missing')) return;
    if (selectedPaths.has(path)) {
      selectedPaths.delete(path);
    } else {
      selectedPaths.add(path);
    }
  }

  function selectAll() {
    selectedPaths.clear();
    for (const e of filtered) if (e.status === 'new' || (e.status === 'modified' && policy === 'update')) selectedPaths.add(e.relative_path);
  }

  function selectNone() {
    // Only deselect entries visible in the current filter
    for (const e of filtered) selectedPaths.delete(e.relative_path);
  }

  function handleSync() {
    if (jobPanelOpen || !scanComplete || !canUseScan(outcome, scanning)) return;
    const selected = allEntries.filter((e) => selectedPaths.has(e.relative_path) && (e.status === 'new' || (e.status === 'modified' && policy === 'update')));
    if (embedded && destBackend === 'local' && selected.some(entry => entry.relative_path.includes('/'))) {
      scanError = 'This guided transfer was refused before copying: the existing local-destination engine cannot preserve selected nested file mappings yet. No files were copied. Use an S3 destination or copy the nested folders separately; local-destination mapping needs a separate engine update.';
      scanComplete = false; outcome = 'failed'; selectedPaths.clear();
      return;
    }
    if (selected.length) onSync(selected);
  }

  function scrollCursorIntoView() {
    if (!listEl) return;
    const row = listEl.children[cursorIndex] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  }

  function handleKeydown(e: KeyboardEvent) {
    // Preset controls own their keyboard interaction. Enter/Space must never
    // bubble into execution or alter the scan selection.
    if ((e.target as HTMLElement | null)?.closest('[data-comparison-presets], [data-saved-jobs]')) {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); }
      return;
    }
    if (e.key !== 'Escape' && (e.target as HTMLElement | null)?.closest('button, input, select, textarea, summary')) return;
    // Don't intercept keys when typing in the exclude input
    if (excludeInput && document.activeElement === excludeInput) {
      if (e.key === 'Escape') {
        e.preventDefault();
        excludeInput.blur();
      }
      return;
    }

    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (scanning && currentSyncId) {
        invalidateScan();
      } else {
        onClose();
      }
      return;
    }

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filtered.length > 0) {
        cursorIndex = Math.min(cursorIndex + 1, filtered.length - 1);
        scrollCursorIntoView();
      }
      return;
    }

    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filtered.length > 0) {
        cursorIndex = Math.max(cursorIndex - 1, 0);
        scrollCursorIntoView();
      }
      return;
    }

    if (e.key === ' ') {
      e.preventDefault();
      const entry = filtered[cursorIndex];
      if (entry) {
        toggleSelection(entry.relative_path);
      }
      return;
    }

    if (e.key === 'Enter') {
      e.preventDefault();
      // Transfers require the explicit review button, never a bubbled Enter.
      return;
    }
  }

  function formatSize(bytes: number): string {
    if (bytes === 0) return '-';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }

  function formatDelta(source: number, dest: number): string {
    if (dest === 0) return `+${formatSize(source)}`;
    if (source === 0) return `-${formatSize(dest)}`;
    const diff = source - dest;
    if (diff === 0) return '0';
    const prefix = diff > 0 ? '+' : '';
    return `${prefix}${formatSize(Math.abs(diff))}`;
  }

  function statusLabel(status: string): string {
    switch (status) {
      case 'new':
        return 'NEW';
      case 'modified':
        return 'MOD';
      case 'deleted':
        return 'ONLY AT DEST';
      default:
        return status.toUpperCase();
    }
  }

  function filterCount(f: string): number {
    switch (f) {
      case 'all':
        return allEntries.filter((e) => e.status !== 'same').length;
      case 'new':
        return newCount || allEntries.filter((e) => e.status === 'new').length;
      case 'modified':
        return modifiedCount || allEntries.filter((e) => e.status === 'modified').length;
      case 'deleted':
        return deletedCount || allEntries.filter((e) => e.status === 'deleted').length;
      default:
        return 0;
    }
  }

  function setFilter(f: 'all' | 'new' | 'modified' | 'deleted') {
    filter = f;
    cursorIndex = 0;
  }

  let _selectedCount = $derived([...selectedPaths].filter((p) => filtered.some((e) => e.relative_path === p)).length);
</script>

<div
  class="dialog-overlay no-select"
  class:embedded
  role={embedded ? 'region' : 'dialog'}
  aria-modal={embedded ? undefined : true}
  aria-label="Compare and review changes"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    {#if !embedded}<div class="dialog-title">Sync: {sourcePath} &rarr; {destPath}</div>{/if}
    <div class="dialog-body">
      {#if !embedded}<div class="sync-roots"><strong>Source:</strong> {sourcePath} ({sourceBackend})<br /><strong>Destination:</strong> {destPath} ({destBackend})</div>
      <ComparisonPresets mode={compareMode} {excludeText} onApply={applyPreset} />
      <SavedSyncJobs defaultLocalPath={sourceBackend === 'local' ? sourcePath : ''} onOpenChange={(open) => { jobPanelOpen = open; }} />
      {/if}
      {#if jobPanelOpen}<p class="hint">Ordinary Sync is disabled while the separate saved-job controls are open. Close that section to return to the current pane operation; closing it also releases/cancels its read-only native session.</p>{/if}
      <p class="hint">Compare first, then select the changes you want to transfer. Nothing is deleted from the destination. Opening this dialog or applying a preset does not start a scan. S3 writes require a separate guarded write review.</p>
      {#if embedded && destBackend === 'local'}<p class="hint">Local-destination limitation: this guide can copy files directly within the compared folder, but refuses nested file selections because the existing engine cannot preserve those mappings yet. No flattening fallback is used.</p>{/if}
      <label>What should be copied? <select bind:value={policy} onchange={invalidateScan}><option value="missing">Add missing files only</option><option value="update">Add missing files and update changed files</option></select></label>
      {#if optionsSaveError}<ErrorNotice error={optionsSaveError} context="Could not save default Sync exclusions; this scan still uses the displayed options" />{/if}
      <!-- Options row -->
      <div class="options-row">
        <div class="option-group">
          <label class="option-label" for="sync-exclude">Exclude:</label>
          <input
            id="sync-exclude"
            type="text"
            class="exclude-input"
           
            bind:this={excludeInput}
            bind:value={excludeText}
            oninput={invalidateScan}
            placeholder=".DS_Store, node_modules/**, *.tmp"
          />
        </div>
        <div class="option-group">
          <span class="option-label">Compare:</span>
          <label class="radio-label">
            <input type="radio" bind:group={compareMode} value="size_mtime" onchange={invalidateScan} />
            Size + Date
          </label>
          <label class="radio-label">
            <input type="radio" bind:group={compareMode} value="sha256" onchange={invalidateScan} />
            SHA-256 <span class="hint">(reads all contents)</span>
          </label>
        </div>
        <button class="rescan-btn" onclick={startScan} disabled={scanning}>{outcome ? 'Rescan' : 'Compare'}</button>
      </div>

      <!-- Filter buttons -->
      <details><summary>Advanced comparison options and limits</summary>
      {#if embedded}<ComparisonPresets mode={compareMode} {excludeText} onApply={applyPreset} />{/if}
      <p class="hint">Scan budgets: {appState.s3ScanLimits.max_objects.toLocaleString()} entries, {appState.s3ScanLimits.max_requests.toLocaleString()} S3 API calls, {appState.s3ScanLimits.max_seconds}s (both sides combined). Configure in Preferences → Network.</p>
      </details>
      {#if compareMode === 'sha256' && (sourceBackend === 's3' || destBackend === 's3')}
        <p class="hint">SHA-256 downloads included S3 contents; API request and download/egress charges may apply.</p>
      {/if}
      {#if scanProgress}<ScanStatus progress={scanProgress} outcome={outcome ?? undefined} message={scanError} />{/if}
      {#if scanError && !scanProgress}<ErrorNotice error={scanError} context="Comparison could not finish" />{/if}
      {#if scanning}<button class="rescan-btn" onclick={invalidateScan}>Cancel scan</button>{/if}
      {#if scanning || outcome}<div class="filter-row">
        <button
          class="filter-btn"
          class:active={filter === 'all'}
          onclick={() => setFilter('all')}
        >All <span class="badge">{filterCount('all')}</span></button>
        <button
          class="filter-btn new"
          class:active={filter === 'new'}
          onclick={() => setFilter('new')}
        >New <span class="badge">{filterCount('new')}</span></button>
        <button
          class="filter-btn modified"
          class:active={filter === 'modified'}
          onclick={() => setFilter('modified')}
        >Modified <span class="badge">{filterCount('modified')}</span></button>
        <button
          class="filter-btn deleted"
          class:active={filter === 'deleted'}
          onclick={() => setFilter('deleted')}
        >Destination only <span class="badge">{filterCount('deleted')}</span></button>

        <span class="spacer"></span>
        <button class="select-btn" onclick={selectAll}>Select All</button>
        <button class="select-btn" onclick={selectNone}>Select None</button>
      </div>

      <!-- Entry list -->
      <div class="entry-list" bind:this={listEl}>
        {#each filtered as entry, i (entry.relative_path)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="entry-row"
            class:cursor={i === cursorIndex}
            onclick={() => toggleSelection(entry.relative_path)}
            onmouseenter={() => { cursorIndex = i; }}
            role="option"
            tabindex="-1"
            aria-selected={selectedPaths.has(entry.relative_path)}
          >
            <input
              type="checkbox"
              class="entry-check"
              disabled={entry.status === 'deleted' || (entry.status === 'modified' && policy === 'missing')}
              checked={selectedPaths.has(entry.relative_path)}
              onclick={(e) => { e.stopPropagation(); toggleSelection(entry.relative_path); }}
              tabindex="-1"
            />
            <span class="entry-status status-{entry.status}">{statusLabel(entry.status)}</span>
            <span class="entry-path">{entry.relative_path}</span>
            <span class="entry-sizes">
              {formatSize(entry.source_size)} &rarr; {formatSize(entry.dest_size)}
            </span>
            <span class="entry-delta">{formatDelta(entry.source_size, entry.dest_size)}</span>
          </div>
        {/each}
        {#if filtered.length === 0 && scanComplete}
          <div class="no-entries">
            {#if filter === 'all'}
              {compareMode === 'sha256' ? 'SHA-256 contents match for all included files' : 'Size and timestamps match (contents not verified)'}
            {:else}
              No {filter} files found
            {/if}
          </div>
        {/if}
      </div>
      {/if}

    </div>
    <!-- Footer -->
    <div class="dialog-footer">
      <span class="status-text">
        {#if scanning}
          Scanning{compareMode === 'sha256' ? ' (reading contents for SHA-256; S3 download charges may apply)' : ''}... ({allEntries.length} files)
        {:else if scanError}
          Comparison incomplete — inspect the diagnostic above. No transfer is authorized.
        {:else if scanComplete}
          {filterCount('new')} new, {filterCount('modified')} modified, {filterCount('deleted')} destination-only (kept) &mdash; {selectedPaths.size} selected
        {/if}
      </span>
      <div class="footer-buttons">
        <button class="dialog-btn" onclick={onClose}>{embedded ? 'Back to destination' : 'Cancel'}</button>
        <button
          class="dialog-btn sync-btn"
          disabled={jobPanelOpen || selectedPaths.size === 0 || !canUseScan(outcome, scanning) || !scanComplete}
          onclick={handleSync}
        >{destBackend === 's3' ? 'Review transfer' : 'Copy selected'} ({selectedPaths.size})</button>
      </div>
    </div>
  </div>
</div>

<style>
  .dialog-overlay.embedded { position: static; display: block; background: none; backdrop-filter: none; -webkit-backdrop-filter: none; }
  .embedded .dialog-box { width: 100%; height: auto; max-width: none; max-height: none; border: 0; border-radius: 0; box-shadow: none; background: transparent; }
  .embedded .dialog-body { padding: 0; overflow: visible; }
  .embedded .entry-list { min-height: 120px; max-height: 35vh; }
  .embedded .dialog-footer { padding: 16px 0 0; }
  .dialog-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    z-index: 100;
  }

  .dialog-box {
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-lg);
    width: 72ch;
    height: 85vh;
    max-width: 90vw;
    max-height: 900px;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dialog-body {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .sync-roots { font-size: 12px; overflow-wrap: anywhere; flex-shrink: 0; }

  /* Options row */
  .options-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    background: var(--bg-surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .option-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .option-label {
    font-size: 12px;
    color: var(--text-secondary);
    flex-shrink: 0;
    min-width: 60px;
  }

  .exclude-input {
    flex: 1;
    padding: 4px 8px;
    font-size: 12px;
    font-family: inherit;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    outline: none;
  }

  .exclude-input:focus {
    border-color: var(--text-accent);
  }

  .exclude-input::placeholder {
    color: var(--text-secondary);
    opacity: 0.5;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .radio-label input[type="radio"] {
    cursor: pointer;
  }

  .hint {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .rescan-btn {
    align-self: flex-end;
    padding: 4px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .rescan-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .rescan-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Filter row */
  .filter-row {
    display: flex;
    gap: 4px;
    align-items: center;
    flex-wrap: wrap;
  }

  .filter-btn {
    padding: 4px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background 0.15s, color 0.15s;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .filter-btn:hover:not(.active) {
    background: var(--bg-hover);
  }

  .filter-btn.active {
    background: rgba(110, 168, 254, 0.2);
    color: var(--text-accent);
    border-color: var(--text-accent);
  }

  .badge {
    background: rgba(255, 255, 255, 0.1);
    padding: 1px 5px;
    border-radius: 8px;
    font-size: 10px;
    min-width: 16px;
    text-align: center;
  }

  .spacer {
    flex: 1;
  }

  .select-btn {
    padding: 4px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .select-btn:hover {
    background: var(--bg-hover);
  }

  /* Entry list */
  .entry-list {
    flex: 1;
    overflow-y: auto;
    max-height: 400px;
    min-height: 100px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-primary);
  }

  .entry-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    cursor: pointer;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    font-size: 12px;
  }

  .entry-row:last-child {
    border-bottom: none;
  }

  .entry-row.cursor {
    background: var(--cursor-bg);
  }

  .entry-row:hover:not(.cursor) {
    background: var(--bg-hover);
  }

  .entry-check {
    flex-shrink: 0;
    cursor: pointer;
  }

  .entry-status {
    flex-shrink: 0;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: uppercase;
    min-width: 32px;
    text-align: center;
  }

  .status-new {
    background: rgba(80, 200, 120, 0.2);
    color: #50c878;
  }

  .status-modified {
    background: rgba(255, 193, 7, 0.2);
    color: #ffc107;
  }

  .status-deleted {
    background: rgba(244, 67, 54, 0.2);
    color: #f44336;
  }

  .entry-path {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-primary);
  }

  .entry-sizes {
    flex-shrink: 0;
    color: var(--text-secondary);
    font-size: 11px;
    white-space: nowrap;
  }

  .entry-delta {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-secondary);
    min-width: 60px;
    text-align: right;
  }

  .no-entries {
    padding: 20px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 13px;
  }

  /* Footer */
  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 12px 20px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .status-text {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .footer-buttons {
    display: flex;
    gap: 8px;
  }

  .dialog-btn {
    padding: 8px 24px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition:
      background var(--transition-fast),
      border-color var(--transition-fast);
  }

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .sync-btn {
    background: rgba(110, 168, 254, 0.15);
    border-color: var(--text-accent);
    color: var(--text-accent);
  }

  .sync-btn:hover:not(:disabled) {
    background: rgba(110, 168, 254, 0.3);
  }

  .sync-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
