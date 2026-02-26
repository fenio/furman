<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { syncDiff, cancelSync } from '$lib/services/tauri';
  import { appState } from '$lib/state/app.svelte';
  import type { SyncEntry, SyncEvent, PanelBackend } from '$lib/types';

  interface Props {
    sourceBackend: PanelBackend;
    sourcePath: string;
    sourceS3Id: string;
    destBackend: PanelBackend;
    destPath: string;
    destS3Id: string;
    onSync: (entries: SyncEntry[]) => void;
    onClose: () => void;
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
  }: Props = $props();

  let allEntries = $state<SyncEntry[]>([]);
  let scanning = $state(false);
  let scanComplete = $state(false);
  let filter = $state<'all' | 'new' | 'modified' | 'deleted'>('all');
  let selectedPaths = $state(new Set<string>());
  let cursorIndex = $state(0);
  let currentSyncId = $state('');
  let listEl: HTMLDivElement | undefined = $state(undefined);
  let excludeInput: HTMLInputElement | undefined = $state(undefined);

  // Options
  let excludeText = $state(appState.syncExcludePatterns);
  let compareMode = $state<'size_mtime' | 'checksum'>('size_mtime');

  // Summary counts
  let newCount = $state(0);
  let modifiedCount = $state(0);
  let deletedCount = $state(0);

  let filtered = $derived.by(() => {
    if (filter === 'all') return allEntries.filter((e) => e.status !== 'same');
    return allEntries.filter((e) => e.status === filter);
  });

  onMount(() => {
    startScan();
  });

  onDestroy(() => {
    if (currentSyncId) {
      cancelSync(currentSyncId).catch(() => {});
    }
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
    allEntries = [];
    selectedPaths = new Set();
    cursorIndex = 0;
    scanning = true;
    scanComplete = false;
    newCount = 0;
    modifiedCount = 0;
    deletedCount = 0;

    // Persist exclude patterns
    if (appState.syncExcludePatterns !== excludeText) {
      appState.syncExcludePatterns = excludeText;
      appState.persistConfig();
    }

    const handleEvent = (event: SyncEvent) => {
      if (id !== currentSyncId) return;

      if (event.type === 'Entry') {
        allEntries = [...allEntries, event as SyncEntry];
        // Auto-select new and modified entries
        if (event.status === 'new' || event.status === 'modified') {
          selectedPaths = new Set([...selectedPaths, event.relative_path]);
        }
      } else if (event.type === 'Done') {
        newCount = event.new_count;
        modifiedCount = event.modified;
        deletedCount = event.deleted;
        scanning = false;
        scanComplete = true;
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
    ).catch(() => {
      if (id === currentSyncId) {
        scanning = false;
        scanComplete = true;
      }
    });
  }

  function toggleSelection(path: string) {
    const next = new Set(selectedPaths);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    selectedPaths = next;
  }

  function selectAll() {
    selectedPaths = new Set(filtered.map((e) => e.relative_path));
  }

  function selectNone() {
    // Only deselect entries visible in the current filter
    const visiblePaths = new Set(filtered.map((e) => e.relative_path));
    selectedPaths = new Set([...selectedPaths].filter((p) => !visiblePaths.has(p)));
  }

  function handleSync() {
    const selected = allEntries.filter((e) => selectedPaths.has(e.relative_path));
    onSync(selected);
  }

  function scrollCursorIntoView() {
    if (!listEl) return;
    const row = listEl.children[cursorIndex] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  }

  function handleKeydown(e: KeyboardEvent) {
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
        cancelSync(currentSyncId).catch(() => {});
        scanning = false;
        scanComplete = true;
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
      if (selectedPaths.size > 0) {
        handleSync();
      }
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
        return 'DEL';
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

  let selectedCount = $derived([...selectedPaths].filter((p) => filtered.some((e) => e.relative_path === p)).length);
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Sync: {sourcePath} &rarr; {destPath}</div>
    <div class="dialog-body">
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
            placeholder=".DS_Store, node_modules/**, *.tmp"
          />
        </div>
        <div class="option-group">
          <span class="option-label">Compare:</span>
          <label class="radio-label">
            <input type="radio" bind:group={compareMode} value="size_mtime" />
            Size + Date
          </label>
          <label class="radio-label">
            <input type="radio" bind:group={compareMode} value="checksum" />
            Checksum <span class="hint">(slower)</span>
          </label>
        </div>
        <button class="rescan-btn" onclick={startScan} disabled={scanning}>Rescan</button>
      </div>

      <!-- Filter buttons -->
      <div class="filter-row">
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
        >Deleted <span class="badge">{filterCount('deleted')}</span></button>

        <span class="spacer"></span>
        <button class="select-btn" onclick={selectAll}>Select All</button>
        <button class="select-btn" onclick={selectNone}>Select None</button>
      </div>

      <!-- Entry list -->
      <div class="entry-list" bind:this={listEl}>
        {#each filtered as entry, i}
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
              Directories are in sync
            {:else}
              No {filter} files found
            {/if}
          </div>
        {/if}
      </div>

    </div>
    <!-- Footer -->
    <div class="dialog-footer">
      <span class="status-text">
        {#if scanning}
          Scanning{compareMode === 'checksum' ? ' (checksumming)' : ''}... ({allEntries.length} files)
        {:else if scanComplete}
          {filterCount('new')} new, {filterCount('modified')} modified, {filterCount('deleted')} deleted &mdash; {selectedPaths.size} selected
        {/if}
      </span>
      <div class="footer-buttons">
        <button class="dialog-btn" onclick={onClose}>Cancel</button>
        <button
          class="dialog-btn sync-btn"
          disabled={selectedPaths.size === 0 || scanning}
          onclick={handleSync}
        >Sync ({selectedPaths.size})</button>
      </div>
    </div>
  </div>
</div>

<style>
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
