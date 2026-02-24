<script lang="ts">
  import type { PanelData } from '$lib/state/panels.svelte.ts';
  import { appState } from '$lib/state/app.svelte.ts';
  import { formatSize } from '$lib/utils/format.ts';
  import { onMount, tick } from 'svelte';
  import FileRow from './FileRow.svelte';
  import FileIcon from './FileIcon.svelte';
  import BreadcrumbBar from './BreadcrumbBar.svelte';

  interface Props {
    panel: PanelData;
    isActive: boolean;
    side?: 'left' | 'right';
    onActivate?: () => void;
    onEntryActivate?: (index: number) => void;
    onDrop?: (sourceSide: 'left' | 'right', shiftKey: boolean) => void;
  }

  let { panel, isActive, side, onActivate, onEntryActivate, onDrop }: Props = $props();

  let listContainer: HTMLDivElement | undefined = $state(undefined);
  let filterInput: HTMLInputElement | undefined = $state(undefined);
  let homePath = $state('');
  let isDragOver = $state(false);

  // Scroll to keep cursor visible
  $effect(() => {
    const idx = panel.cursorIndex;
    if (!listContainer) return;

    if (panel.viewMode === 'icon') {
      const tile = listContainer.querySelectorAll('.file-tile')[idx] as HTMLElement | undefined;
      tile?.scrollIntoView({ block: 'nearest' });
    } else {
      const rowHeight = listContainer.querySelector('.file-row')?.getBoundingClientRect().height ?? 19.5;
      const scrollTop = listContainer.scrollTop;
      const viewHeight = listContainer.clientHeight;
      const rowTop = idx * rowHeight;
      const rowBottom = rowTop + rowHeight;

      if (rowTop < scrollTop) {
        listContainer.scrollTop = rowTop;
      } else if (rowBottom > scrollTop + viewHeight) {
        listContainer.scrollTop = rowBottom - viewHeight;
      }
    }
  });

  // Measure grid columns for keyboard navigation
  $effect(() => {
    if (panel.viewMode !== 'icon' || !listContainer) {
      panel.gridColumns = 1;
      return;
    }
    const observer = new ResizeObserver(() => {
      if (listContainer) {
        const cols = getComputedStyle(listContainer).gridTemplateColumns.split(' ').length;
        panel.gridColumns = cols;
      }
    });
    observer.observe(listContainer);
    return () => observer.disconnect();
  });

  // Clamp cursor when filtered list length changes
  $effect(() => {
    const len = panel.filteredSortedEntries.length;
    if (len > 0 && panel.cursorIndex >= len) {
      panel.cursorIndex = len - 1;
    }
  });

  // Auto-focus filter input when it appears
  $effect(() => {
    if (panel.filterText && filterInput) {
      tick().then(() => filterInput?.focus());
    }
  });

  function handleColumnClick(field: 'name' | 'size' | 'modified' | 'extension') {
    panel.toggleSort(field);
  }

  function handleRowClick(index: number) {
    onActivate?.();
    panel.moveCursorTo(index);
  }

  function handleRowDblClick(index: number) {
    onActivate?.();
    panel.moveCursorTo(index);
    onEntryActivate?.(index);
  }

  function sortIndicator(field: string): string {
    if (panel.sortField !== field) return '';
    return panel.sortDirection === 'asc' ? ' ▲' : ' ▼';
  }

  onMount(async () => {
    try {
      const { homeDir } = await import('@tauri-apps/api/path');
      homePath = (await homeDir()).replace(/\/+$/, '');
    } catch {
      homePath = '';
    }
  });

  const isS3 = $derived(panel.backend === 's3');
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="file-panel no-select"
  class:active={isActive}
  class:drag-over={isDragOver}
  role="region"
  onclick={() => onActivate?.()}
  ondragover={(e) => {
    if (e.dataTransfer?.types.includes('application/x-furman-files')) {
      e.preventDefault();
      e.dataTransfer.dropEffect = e.shiftKey ? 'move' : 'copy';
      isDragOver = true;
    }
  }}
  ondragleave={(e) => {
    const related = e.relatedTarget as Node | null;
    if (related && e.currentTarget instanceof Node && e.currentTarget.contains(related)) return;
    isDragOver = false;
  }}
  ondrop={(e) => {
    e.preventDefault();
    isDragOver = false;
    const raw = e.dataTransfer?.getData('application/x-furman-files');
    if (!raw) return;
    try {
      const data = JSON.parse(raw);
      if (data.side && data.side !== side) {
        onDrop?.(data.side, e.shiftKey);
      }
    } catch { /* ignore */ }
  }}
>
  <!-- Header: breadcrumb path -->
  <div class="panel-header" class:has-s3-disconnect={panel.backend === 's3'}>
    {#if panel.backend === 's3'}
      <button class="s3-disconnect" onclick={() => panel.disconnectS3(homePath)} title="Disconnect from S3">
        &times;
      </button>
    {/if}
    <BreadcrumbBar
      path={panel.path}
      backend={panel.backend}
      s3Connection={panel.s3Connection}
      archiveInfo={panel.archiveInfo}
      {homePath}
      onNavigate={(p) => panel.loadDirectory(p)}
    />
    <button class="view-toggle" onclick={() => panel.toggleViewMode()} title={panel.viewMode === 'list' ? 'Switch to icon view' : 'Switch to list view'}>
      {panel.viewMode === 'list' ? '\u229E' : '\u2630'}
    </button>
  </div>

  <!-- Column headers -->
  {#if panel.viewMode === 'list'}
  <div class="column-headers">
    <button class="col-header col-name" onclick={() => handleColumnClick('name')}>
      Name{sortIndicator('name')}
    </button>
    <button class="col-header col-size" onclick={() => handleColumnClick('size')}>
      Size{sortIndicator('size')}
    </button>
    <button class="col-header col-date" onclick={() => handleColumnClick('modified')}>
      Date{sortIndicator('modified')}
    </button>
    <button class="col-header col-perm" onclick={() => handleColumnClick('extension')}>
      Ext{sortIndicator('extension')}
    </button>
  </div>
  {/if}

  <!-- Filter bar -->
  {#if panel.filterText}
  <div class="filter-bar">
    <span class="filter-icon">🔍</span>
    <input
      bind:this={filterInput}
      bind:value={panel.filterText}
      class="filter-input"
      placeholder="Filter..."
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation();
          panel.clearFilter();
        }
      }}
    />
  </div>
  {/if}

  <!-- File list -->
  <div
    class="file-list"
    class:icon-grid={panel.viewMode === 'icon'}
    bind:this={listContainer}
    style={panel.viewMode === 'icon' ? `--icon-size: ${appState.iconSize}px; --grid-min: ${appState.iconSize + 32}px` : ''}
  >
    {#if panel.loading}
      <div class="loading-msg">Loading...</div>
    {:else if panel.error}
      <div class="error-msg">{panel.error}</div>
    {:else}
      {#each panel.filteredSortedEntries as entry, i (entry.path + entry.name)}
        {#if panel.viewMode === 'icon'}
          <FileIcon
            {entry}
            isSelected={panel.selectedPaths.has(entry.path)}
            isCursor={i === panel.cursorIndex}
            {isActive}
            panelSide={side}
            onclick={() => handleRowClick(i)}
            ondblclick={() => handleRowDblClick(i)}
          />
        {:else}
          <FileRow
            {entry}
            isSelected={panel.selectedPaths.has(entry.path)}
            isCursor={i === panel.cursorIndex}
            {isActive}
            rowIndex={i}
            panelSide={side}
            onclick={() => handleRowClick(i)}
            ondblclick={() => handleRowDblClick(i)}
          />
        {/if}
      {/each}
    {/if}
  </div>

  <!-- Footer: selection info or current file info -->
  <div class="panel-footer">
    {#if panel.selectedCount > 0}
      <span>{panel.selectedCount} selected ({formatSize(panel.selectedSize)})</span>
    {:else if panel.currentEntry}
      <span>
        {panel.currentEntry.name}
        {#if !panel.currentEntry.is_dir}
          ({formatSize(panel.currentEntry.size)})
        {/if}
      </span>
    {:else}
      <span>&nbsp;</span>
    {/if}
    <span class="free-space">{isS3 ? `S3 ${panel.s3Connection?.bucket ?? ''}` : `${formatSize(panel.freeSpace)} free`}</span>
  </div>
</div>

<style>
  .file-panel {
    display: flex;
    flex-direction: column;
    flex: 1 1 50%;
    min-width: 0;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    overflow: hidden;
    border-radius: var(--radius-md);
    margin: 4px;
  }

  .file-panel.active {
    border-color: var(--border-active);
  }

  .file-panel.drag-over {
    border-color: var(--success-color);
    background: color-mix(in srgb, var(--success-color) 5%, var(--bg-panel));
  }

  .panel-header {
    position: relative;
    background: var(--header-bg);
    color: var(--header-text);
    padding: 6px 12px;
    border-bottom: 1px solid var(--border-subtle);
    white-space: nowrap;
    overflow: hidden;
    flex: 0 0 auto;
    opacity: 0.7;
  }

  .panel-header.has-s3-disconnect {
    padding-left: 28px;
  }

  .s3-disconnect {
    position: absolute;
    left: 6px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--error-color);
    font-size: 16px;
    cursor: pointer;
    padding: 2px 4px;
    line-height: 1;
    opacity: 0.6;
    transition: opacity var(--transition-fast);
    z-index: 1;
  }

  .s3-disconnect:hover {
    opacity: 1;
  }

  .view-toggle {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    color: var(--header-text);
    font-size: 16px;
    cursor: pointer;
    padding: 2px 4px;
    line-height: 1;
    opacity: 0.6;
    transition: opacity var(--transition-fast);
  }

  .view-toggle:hover {
    opacity: 1;
  }

  .active .panel-header {
    opacity: 1;
    border-bottom-color: var(--border-active);
  }

  .column-headers {
    display: flex;
    flex-direction: row;
    background: var(--bg-header);
    color: var(--text-accent);
    border-bottom: 1px solid var(--border-subtle);
    flex: 0 0 auto;
    padding: 4px 8px;
  }

  .col-header {
    color: var(--text-accent);
    text-align: left;
    cursor: pointer;
    line-height: 1.6em;
    font-size: 12px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    opacity: 0.5;
    transition: opacity var(--transition-fast);
  }

  .col-header:hover {
    opacity: 1;
    color: var(--border-active);
  }

  .col-header.col-name {
    flex: 1 1 0;
    min-width: 0;
  }

  .col-header.col-size {
    flex: 0 0 9ch;
    text-align: right;
    padding-right: 1ch;
  }

  .col-header.col-date {
    flex: 0 0 16ch;
    text-align: left;
    padding-right: 1ch;
  }

  .col-header.col-perm {
    flex: 0 0 9ch;
    text-align: left;
  }

  .filter-bar {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: var(--bg-surface);
    border-bottom: 1px solid var(--border-subtle);
    flex: 0 0 auto;
  }

  .filter-icon {
    font-size: 12px;
    opacity: 0.6;
  }

  .filter-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
  }

  .file-list {
    flex: 1 1 0;
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    padding: 4px 0;
  }

  .file-list.icon-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--grid-min, 80px), 1fr));
    gap: 4px;
    padding: 8px;
    align-content: start;
  }

  .loading-msg,
  .error-msg {
    padding: 8px 12px;
    color: var(--text-secondary);
  }

  .error-msg {
    color: var(--error-color);
  }

  .panel-footer {
    display: flex;
    justify-content: space-between;
    background: var(--footer-bg);
    color: var(--footer-text);
    padding: 4px 12px;
    border-top: 1px solid var(--border-subtle);
    white-space: nowrap;
    overflow: hidden;
    flex: 0 0 auto;
    font-size: 12px;
    opacity: 0.6;
  }

  .active .panel-footer {
    border-top-color: var(--border-active);
    opacity: 1;
  }

  .free-space {
    color: var(--text-secondary);
  }
</style>
