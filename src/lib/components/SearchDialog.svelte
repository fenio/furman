<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { searchFiles, cancelSearch } from '$lib/services/tauri';
  import type { SearchResult, SearchEvent, SearchMode } from '$lib/types';

  interface Props {
    root: string;
    onNavigate: (dirPath: string, fileName: string) => void;
    onClose: () => void;
  }

  let { root, onNavigate, onClose }: Props = $props();

  let query = $state('');
  let mode = $state<SearchMode>('name');
  let results = $state<SearchResult[]>([]);
  let cursorIndex = $state(0);
  let searching = $state(false);
  let totalFound = $state(0);
  let searchComplete = $state(false);
  let currentSearchId = $state('');
  let inputEl: HTMLInputElement | undefined = $state(undefined);
  let resultsEl: HTMLDivElement | undefined = $state(undefined);
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    inputEl?.focus();
  });

  onDestroy(() => {
    clearTimeout(debounceTimer);
    if (currentSearchId) {
      cancelSearch(currentSearchId).catch(() => {});
    }
  });

  function relativePath(fullPath: string): string {
    if (fullPath.startsWith(root)) {
      const rel = fullPath.slice(root.length);
      return rel.startsWith('/') ? rel.slice(1) : rel;
    }
    return fullPath;
  }

  function dirOfPath(fullPath: string): string {
    const idx = fullPath.lastIndexOf('/');
    return idx > 0 ? fullPath.slice(0, idx) : '/';
  }

  function fileOfPath(fullPath: string): string {
    const idx = fullPath.lastIndexOf('/');
    return idx >= 0 ? fullPath.slice(idx + 1) : fullPath;
  }

  async function doSearch() {
    // Cancel any previous search.
    if (currentSearchId) {
      await cancelSearch(currentSearchId).catch(() => {});
    }

    if (query.length < 2) {
      results = [];
      searching = false;
      searchComplete = false;
      totalFound = 0;
      currentSearchId = '';
      return;
    }

    const id = `search-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    currentSearchId = id;
    results = [];
    cursorIndex = 0;
    totalFound = 0;
    searchComplete = false;
    searching = true;

    searchFiles(id, root, query, mode, (event: SearchEvent) => {
      // Ignore events from stale searches.
      if (id !== currentSearchId) return;

      if (event.type === 'Result') {
        results = [...results, event as SearchResult];
        totalFound = results.length;
      } else if (event.type === 'Done') {
        totalFound = event.total_found;
        searchComplete = true;
        searching = false;
      }
    }).catch(() => {
      if (id === currentSearchId) {
        searching = false;
        searchComplete = true;
      }
    });
  }

  function handleInput() {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      doSearch();
    }, 300);
  }

  function handleModeChange(newMode: SearchMode) {
    mode = newMode;
    // Re-trigger search with the new mode.
    clearTimeout(debounceTimer);
    doSearch();
  }

  function activateResult(index: number) {
    const r = results[index];
    if (!r) return;
    onNavigate(dirOfPath(r.path), fileOfPath(r.path));
  }

  function scrollCursorIntoView() {
    if (!resultsEl) return;
    const row = resultsEl.children[cursorIndex] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (searching && currentSearchId) {
        cancelSearch(currentSearchId).catch(() => {});
        searching = false;
        searchComplete = true;
      } else {
        onClose();
      }
      return;
    }

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (results.length > 0) {
        cursorIndex = Math.min(cursorIndex + 1, results.length - 1);
        scrollCursorIntoView();
      }
      return;
    }

    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (results.length > 0) {
        cursorIndex = Math.max(cursorIndex - 1, 0);
        scrollCursorIntoView();
      }
      return;
    }

    if (e.key === 'Enter') {
      e.preventDefault();
      if (results.length > 0) {
        activateResult(cursorIndex);
      }
      return;
    }
  }

  function statusText(): string {
    if (searching) {
      return `Searching... (${results.length} found)`;
    }
    if (!searchComplete) return '';
    if (results.length < totalFound) {
      return `${results.length} shown of ${totalFound} total matches`;
    }
    return `${totalFound} match${totalFound === 1 ? '' : 'es'} found`;
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div class="dialog-box">
    <div class="dialog-title">Search in {root}</div>
    <div class="dialog-body">
      <div class="search-row">
        <input
          type="text"
          class="dialog-input search-input"
          bind:value={query}
          bind:this={inputEl}
          oninput={handleInput}
          placeholder={mode === 'name' ? 'File or directory name...' : 'Search file contents...'}
        />
      </div>

      <div class="mode-toggle">
        <button
          class="mode-btn"
          class:active={mode === 'name'}
          onclick={() => handleModeChange('name')}
        >Name</button>
        <button
          class="mode-btn"
          class:active={mode === 'content'}
          onclick={() => handleModeChange('content')}
        >Content</button>
      </div>

      <div class="results-list" bind:this={resultsEl}>
        {#each results as r, i}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="result-row"
            class:selected={i === cursorIndex}
            onclick={() => activateResult(i)}
            onmouseenter={() => { cursorIndex = i; }}
            role="option"
            tabindex="-1"
            aria-selected={i === cursorIndex}
          >
            <div class="result-main">
              <span class="result-icon">{r.is_dir ? '/' : ''}</span>
              <span class="result-name">{r.name}</span>
              <span class="result-size">{formatSize(r.size)}</span>
            </div>
            <div class="result-path">{relativePath(r.path)}</div>
            {#if r.snippet}
              <div class="result-snippet">
                <span class="result-line">L{r.line_number}:</span> {r.snippet}
              </div>
            {/if}
          </div>
        {/each}
        {#if results.length === 0 && searchComplete}
          <div class="no-results">No matches found</div>
        {/if}
      </div>

      <div class="search-footer">
        <span class="status-text">{statusText()}</span>
        <button class="dialog-btn" onclick={onClose}>Close</button>
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
    width: 60ch;
    max-width: 90vw;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    max-height: 80vh;
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
    min-height: 0;
  }

  .search-row {
    display: flex;
    gap: 8px;
  }

  .search-input {
    flex: 1;
    width: 100%;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    font-family: inherit;
    font-size: 14px;
    box-sizing: border-box;
  }

  .search-input:focus {
    border-color: var(--border-active);
    box-shadow: 0 0 0 1px rgba(110,168,254,0.3);
  }

  .mode-toggle {
    display: flex;
    gap: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    overflow: hidden;
    align-self: flex-start;
  }

  .mode-btn {
    padding: 5px 16px;
    border: none;
    background: var(--bg-surface);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background 0.15s, color 0.15s;
  }

  .mode-btn:not(:last-child) {
    border-right: 1px solid var(--border-subtle);
  }

  .mode-btn.active {
    background: rgba(110,168,254,0.2);
    color: var(--text-accent);
  }

  .mode-btn:hover:not(.active) {
    background: var(--bg-hover);
  }

  .results-list {
    flex: 1;
    overflow-y: auto;
    max-height: 400px;
    min-height: 100px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-primary);
  }

  .result-row {
    padding: 6px 10px;
    cursor: pointer;
    border-bottom: 1px solid rgba(255,255,255,0.03);
  }

  .result-row:last-child {
    border-bottom: none;
  }

  .result-row.selected {
    background: var(--cursor-bg);
  }

  .result-row:hover:not(.selected) {
    background: var(--bg-hover);
  }

  .result-main {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }

  .result-icon {
    color: var(--text-accent);
    width: 1ch;
    flex-shrink: 0;
  }

  .result-name {
    color: var(--text-primary);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .result-size {
    color: var(--text-secondary);
    font-size: 11px;
    margin-left: auto;
    flex-shrink: 0;
  }

  .result-path {
    color: var(--text-secondary);
    font-size: 11px;
    margin-top: 1px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .result-snippet {
    color: var(--text-secondary);
    font-size: 11px;
    margin-top: 2px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: 'SF Mono', 'Menlo', monospace;
  }

  .result-line {
    color: var(--text-accent);
    font-weight: 500;
  }

  .no-results {
    padding: 20px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .search-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 2px;
  }

  .status-text {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .dialog-btn {
    padding: 8px 24px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }
</style>
