<script lang="ts">
  import type { PanelData } from '$lib/state/panels.svelte';
  import type { SortField, ColumnId } from '$lib/types';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import { appState } from '$lib/state/app.svelte';
  import { statusState } from '$lib/state/status.svelte';
  import { formatSize } from '$lib/utils/format';
  import { platform } from '$lib/state/platform.svelte';
  import { gitPull, getGitRepoInfo, gitListBranches, gitCheckout } from '$lib/services/tauri';
  import { onMount, tick } from 'svelte';
  import FileRow from './FileRow.svelte';
  import FileIcon from './FileIcon.svelte';
  import ColumnView from './ColumnView.svelte';
  import BreadcrumbBar from './BreadcrumbBar.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import { comparisonState, type ComparisonStatus } from '$lib/state/comparison.svelte';
  import { ALL_COLUMNS } from '$lib/utils/columns';

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

  // Virtualized scrolling for list mode
  const ROW_HEIGHT_MAP = { compact: 22, normal: 28, comfortable: 34 } as const;
  const ROW_HEIGHT = $derived(ROW_HEIGHT_MAP[appState.rowHeight]);
  const BUFFER_ROWS = 10; // extra rows above/below viewport
  let viewportHeight = $state(0);
  let scrollTop = $state(0);
  let containerWidth = $state(0);

  const totalHeight = $derived(panel.filteredSortedEntries.length * ROW_HEIGHT);
  const startIndex = $derived(
    Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - BUFFER_ROWS)
  );
  const endIndex = $derived(
    Math.min(
      panel.filteredSortedEntries.length,
      Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + BUFFER_ROWS
    )
  );
  const visibleEntries = $derived(
    panel.filteredSortedEntries.slice(startIndex, endIndex)
  );
  const offsetY = $derived(startIndex * ROW_HEIGHT);

  // Virtualized icon grid
  const ICON_TILE_HEIGHT = $derived(appState.iconSize + 40); // icon + label + padding
  const ICON_TILE_WIDTH = $derived(appState.iconSize + 32);
  const iconCols = $derived(Math.max(1, Math.floor(containerWidth / ICON_TILE_WIDTH)));
  const iconTotalRows = $derived(Math.ceil(panel.filteredSortedEntries.length / iconCols));
  const iconTotalHeight = $derived(iconTotalRows * ICON_TILE_HEIGHT);
  const ICON_BUFFER_ROWS = 2;
  const iconStartRow = $derived(
    Math.max(0, Math.floor(scrollTop / ICON_TILE_HEIGHT) - ICON_BUFFER_ROWS)
  );
  const iconEndRow = $derived(
    Math.min(iconTotalRows, Math.ceil((scrollTop + viewportHeight) / ICON_TILE_HEIGHT) + ICON_BUFFER_ROWS)
  );
  const iconStartIndex = $derived(iconStartRow * iconCols);
  const iconEndIndex = $derived(Math.min(panel.filteredSortedEntries.length, iconEndRow * iconCols));
  const iconVisibleEntries = $derived(
    panel.filteredSortedEntries.slice(iconStartIndex, iconEndIndex)
  );
  const iconOffsetY = $derived(iconStartRow * ICON_TILE_HEIGHT);

  function handleListScroll(e: Event) {
    const el = e.target as HTMLElement;
    scrollTop = el.scrollTop;
    containerWidth = el.clientWidth;
  }

  // Context menu state
  let contextMenuOpen = $state(false);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let contextMenuOnEmpty = $state(false);

  // Rubber-band (marquee) selection state
  let rubberBanding = $state(false);
  let rubberStart = $state({ x: 0, y: 0 });
  let rubberCurrent = $state({ x: 0, y: 0 });

  // Scroll to keep cursor visible
  $effect(() => {
    const idx = panel.cursorIndex;
    if (!listContainer || panel.viewMode === 'column') return;

    if (panel.viewMode === 'icon') {
      // Virtualized icon mode: compute cursor position mathematically
      const row = Math.floor(idx / iconCols);
      const rowTop = row * ICON_TILE_HEIGHT;
      const rowBottom = rowTop + ICON_TILE_HEIGHT;
      const scrollTop_ = listContainer.scrollTop;
      const viewHeight = listContainer.clientHeight;

      if (rowTop < scrollTop_) {
        listContainer.scrollTop = rowTop;
      } else if (rowBottom > scrollTop_ + viewHeight) {
        listContainer.scrollTop = rowBottom - viewHeight;
      }
    } else {
      const rowHeight = ROW_HEIGHT;
      const scrollTop_ = listContainer.scrollTop;
      const viewHeight = listContainer.clientHeight;
      const rowTop = idx * rowHeight;
      const rowBottom = rowTop + rowHeight;

      if (rowTop < scrollTop_) {
        listContainer.scrollTop = rowTop;
      } else if (rowBottom > scrollTop_ + viewHeight) {
        listContainer.scrollTop = rowBottom - viewHeight;
      }
    }
  });

  // Measure grid columns for keyboard navigation
  $effect(() => {
    if (panel.viewMode === 'column') return; // ColumnView sets gridColumns itself
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

  // Track viewport height and width for virtualization (list + icon modes)
  $effect(() => {
    if ((panel.viewMode !== 'list' && panel.viewMode !== 'icon') || !listContainer) return;
    const observer = new ResizeObserver((entries) => {
      viewportHeight = entries[0].contentRect.height;
      containerWidth = entries[0].contentRect.width;
    });
    observer.observe(listContainer);
    return () => observer.disconnect();
  });

  // Check encryption status when cursor moves on S3 panel
  $effect(() => {
    const entry = panel.currentEntry;
    if (entry && !entry.is_dir && entry.name !== '..' && panel.backend === 's3') {
      panel.checkEncryption(entry.path);
    }
  });

  // Clamp cursor when filtered list length changes
  $effect(() => {
    const len = panel.filteredSortedEntries.length;
    if (len > 0 && panel.cursorIndex >= len) {
      panel.cursorIndex = len - 1;
    }
  });

  // Compute directory sizes when selection changes (debounced)
  let dirSizeTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    // Access selectedPaths to create a reactive dependency
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    panel.selectedPaths;
    if (dirSizeTimer) clearTimeout(dirSizeTimer);
    dirSizeTimer = setTimeout(() => panel.computeSelectedDirSizes(), 500);
  });

  // Auto-focus filter input when it appears
  $effect(() => {
    if (panel.filterText && filterInput) {
      tick().then(() => filterInput?.focus());
    }
  });

  // Comparison: derive a map from entry name → ComparisonStatus for this panel
  // Pre-builds dirty-directory sets from statusMap (O(M)), then each entry lookup is O(1).
  const comparisonStatusMap = $derived.by((): Map<string, ComparisonStatus> => {
    if (!comparisonState.active) return new SvelteMap();
    const statusMap = side === 'right' ? comparisonState.rightStatuses : comparisonState.leftStatuses;

    // Pre-build sets of top-level directories that have modified/changed children
    const dirModified = new Set<string>();
    const dirChanged = new Set<string>();
    for (const [key, val] of statusMap) {
      if (val === 'same') continue;
      const slashIdx = key.indexOf('/');
      if (slashIdx > 0) {
        const topDir = key.substring(0, slashIdx);
        if (val === 'modified') dirModified.add(topDir);
        else dirChanged.add(topDir);
      }
    }

    const result = new SvelteMap<string, ComparisonStatus>();
    for (const entry of panel.filteredSortedEntries) {
      if (entry.name === '..') continue;
      const status = statusMap.get(entry.name);
      if (status) {
        result.set(entry.name, status);
      } else if (entry.is_dir) {
        if (dirModified.has(entry.name) || dirChanged.has(entry.name)) {
          result.set(entry.name, 'modified');
        }
      }
    }
    return result;
  });

  function handleColumnClick(field: SortField) {
    panel.toggleSort(field);
  }

  function handleRowClick(index: number, e: MouseEvent) {
    onActivate?.();
    if (e.shiftKey) {
      // Range select from anchor to clicked index
      panel.cursorIndex = index;
      panel.selectRange(panel.selectionAnchor, index);
    } else if (e.metaKey || e.ctrlKey) {
      // Toggle selection of clicked item
      panel.cursorIndex = index;
      panel.selectionAnchor = index;
      const entry = panel.filteredSortedEntries[index];
      if (entry && entry.name !== '..') {
        panel.toggleSelection(entry.path);
      }
    } else {
      // Plain click — select only this item
      panel.moveCursorTo(index);
      const entry = panel.filteredSortedEntries[index];
      if (entry && entry.name !== '..') {
        panel.selectedPaths = new Set([entry.path]);
      } else {
        panel.deselectAll();
      }
    }
  }

  function handleRowDblClick(index: number) {
    onActivate?.();
    panel.moveCursorTo(index);
    onEntryActivate?.(index);
  }

  function handleContextMenu(index: number, e: MouseEvent) {
    e.preventDefault();
    onActivate?.();
    panel.moveCursorTo(index);
    const entry = panel.filteredSortedEntries[index];
    if (entry && entry.name !== '..' && !panel.selectedPaths.has(entry.path)) {
      panel.selectedPaths = new Set([entry.path]);
    }
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    contextMenuOnEmpty = false;
    contextMenuOpen = true;
  }

  function handleEmptyContextMenu(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('.file-row') || target.closest('.file-tile') || target.closest('.column-entry')) return;
    e.preventDefault();
    onActivate?.();
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    contextMenuOnEmpty = true;
    contextMenuOpen = true;
  }

  function handleContextMenuAction(key: string) {
    contextMenuOpen = false;
    const keyMap: Record<string, string> = {
      'open': 'Enter',
      'view': 'F3',
      'edit': 'F4',
      'copy': 'F5',
      'move': 'F6',
      'rename': 'F2',
      'delete': 'F8',
      'mkdir': 'F7',
      'properties': 'F9',
    };
    const fKey = keyMap[key];
    if (fKey) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: fKey }));
      return;
    }
    // Clipboard actions via keyboard simulation
    if (key === 'clipboard-copy') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'C', code: 'KeyC', metaKey: true, shiftKey: true }));
      return;
    }
    if (key === 'clipboard-cut') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'X', code: 'KeyX', metaKey: true, shiftKey: true }));
      return;
    }
    if (key === 'clipboard-paste') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'V', code: 'KeyV', metaKey: true, shiftKey: true }));
      return;
    }
    // Disk usage: for ".." analyze the current directory, for other dirs analyze that dir
    if (key === 'disk-usage') {
      const entry = panel.currentEntry;
      if (entry && entry.is_dir && panel.backend === 'local') {
        const targetPath = entry.name === '..' ? panel.path : entry.path;
        window.dispatchEvent(new CustomEvent('disk-usage-request', { detail: targetPath }));
      }
      return;
    }
    // S3-specific actions via custom event
    if (key === 'presign' || key === 'copy-uri' || key === 'bulk-storage') {
      window.dispatchEvent(new CustomEvent('context-action', { detail: key }));
    }
  }

  function getContentCoords(e: MouseEvent) {
    if (!listContainer) return { x: 0, y: 0 };
    const rect = listContainer.getBoundingClientRect();
    return {
      x: e.clientX - rect.left,
      y: e.clientY - rect.top + listContainer.scrollTop,
    };
  }

  function updateRubberBandSelection(prevSelected: Set<string>) {
    if (!listContainer) return;

    const minX = Math.min(rubberStart.x, rubberCurrent.x);
    const maxX = Math.max(rubberStart.x, rubberCurrent.x);
    const minY = Math.min(rubberStart.y, rubberCurrent.y);
    const maxY = Math.max(rubberStart.y, rubberCurrent.y);

    const next = new SvelteSet(prevSelected);

    if (panel.viewMode === 'icon') {
      // Icon mode: pure math using grid geometry (virtualized)
      const tileW = ICON_TILE_WIDTH;
      const tileH = ICON_TILE_HEIGHT;
      const cols = iconCols;
      const totalEntries = panel.filteredSortedEntries.length;

      for (let i = 0; i < totalEntries; i++) {
        const row = Math.floor(i / cols);
        const col = i % cols;
        const tileTop = row * tileH;
        const tileLeft = col * tileW;
        const tileBottom = tileTop + tileH;
        const tileRight = tileLeft + tileW;
        const intersects = !(tileRight < minX || tileLeft > maxX || tileBottom < minY || tileTop > maxY);
        const entry = panel.filteredSortedEntries[i];
        if (!entry || entry.name === '..') continue;
        if (intersects) next.add(entry.path);
        else if (!prevSelected.has(entry.path)) next.delete(entry.path);
      }
    } else {
      // List mode: pure math, no DOM queries (works with virtualization)
      const containerWidth = listContainer.clientWidth;
      const firstRow = Math.max(0, Math.floor(minY / ROW_HEIGHT));
      const lastRow = Math.min(panel.filteredSortedEntries.length - 1, Math.floor(maxY / ROW_HEIGHT));

      for (let i = firstRow; i <= lastRow; i++) {
        const rowTop = i * ROW_HEIGHT;
        const rowBottom = rowTop + ROW_HEIGHT;
        const intersects = !(containerWidth < minX || 0 > maxX || rowBottom < minY || rowTop > maxY);
        const entry = panel.filteredSortedEntries[i];
        if (!entry || entry.name === '..') continue;
        if (intersects) next.add(entry.path);
        else if (!prevSelected.has(entry.path)) next.delete(entry.path);
      }
    }

    panel.selectedPaths = next;
  }

  function handleListMouseDown(e: MouseEvent) {
    // No rubber band in column mode
    if (panel.viewMode === 'column') return;
    // Only start rubber band if clicking on empty space
    const target = e.target as HTMLElement;
    if (target.closest('.file-row') || target.closest('.file-tile')) return;
    if (e.button !== 0) return;

    e.preventDefault();
    onActivate?.();

    const coords = getContentCoords(e);
    rubberStart = coords;
    rubberCurrent = coords;
    rubberBanding = true;

    const prevSelected = (e.metaKey || e.ctrlKey) ? new SvelteSet(panel.selectedPaths) : new SvelteSet<string>();
    if (!(e.metaKey || e.ctrlKey)) {
      panel.deselectAll();
    }

    function onMove(ev: MouseEvent) {
      rubberCurrent = getContentCoords(ev);
      updateRubberBandSelection(prevSelected);
    }

    function onUp() {
      rubberBanding = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
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

  let pulling = $state(false);
  let branchPickerOpen = $state(false);
  let branchList = $state<string[]>([]);
  let branchPickerEl: HTMLDivElement | undefined = $state(undefined);

  async function handleBranchClick(e: MouseEvent) {
    e.stopPropagation();
    if (branchPickerOpen) {
      branchPickerOpen = false;
      return;
    }
    try {
      branchList = await gitListBranches(panel.path);
      branchPickerOpen = true;
      // Close on outside click
      const onClickOutside = (ev: MouseEvent) => {
        if (branchPickerEl && !branchPickerEl.contains(ev.target as Node)) {
          branchPickerOpen = false;
          window.removeEventListener('mousedown', onClickOutside);
        }
      };
      // Delay to avoid catching the current click
      setTimeout(() => window.addEventListener('mousedown', onClickOutside), 0);
    } catch {
      // ignore
    }
  }

  async function handleBranchSelect(branch: string) {
    branchPickerOpen = false;
    if (branch === panel.gitInfo?.branch) return;
    try {
      const result = await gitCheckout(panel.path, branch);
      statusState.setMessage(result || `Switched to ${branch}`);
      const info = await getGitRepoInfo(panel.path);
      panel.gitInfo = info;
      await panel.loadDirectory(panel.path);
    } catch (err: unknown) {
      appState.showAlert(`Checkout failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function handleGitPull() {
    if (pulling) return;
    pulling = true;
    try {
      const result = await gitPull(panel.path);
      statusState.setMessage(result.trim() || 'Pull complete');
      // Refresh git info and panel entries
      const info = await getGitRepoInfo(panel.path);
      panel.gitInfo = info;
      await panel.loadDirectory(panel.path);
    } catch (err: unknown) {
      appState.showAlert(`Pull failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      pulling = false;
    }
  }

  const isS3 = $derived(panel.backend === 's3');

  // Column configuration
  const activeColumns = $derived(ALL_COLUMNS.filter(col => appState.visibleColumns.includes(col.id)));

  let columnPickerOpen = $state(false);
  let columnPickerX = $state(0);
  let columnPickerY = $state(0);

  function handleColumnHeaderContextMenu(e: MouseEvent) {
    e.preventDefault();
    columnPickerX = e.clientX;
    columnPickerY = e.clientY;
    columnPickerOpen = true;
  }

  function toggleColumn(id: ColumnId) {
    if (id === 'name') return; // Name is always visible
    const current = [...appState.visibleColumns];
    const idx = current.indexOf(id);
    if (idx >= 0) {
      current.splice(idx, 1);
    } else {
      current.push(id);
    }
    appState.setVisibleColumns(current);
  }
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
    if (e.dataTransfer?.types.includes('application/x-furman-files') ||
        e.dataTransfer?.types.includes('Files')) {
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
    if (raw) {
      try {
        const data = JSON.parse(raw);
        if (data.side && data.side !== side) {
          onDrop?.(data.side, e.shiftKey);
        }
      } catch { /* ignore */ }
    }
    // Native file drops are handled by Tauri's onDragDropEvent in +layout.svelte
  }}
>
  <!-- Header: breadcrumb path -->
  <div class="panel-header" class:has-s3-disconnect={panel.backend === 's3' || panel.backend === 'sftp'}>
    {#if panel.backend === 's3'}
      <button class="s3-disconnect" onclick={() => panel.disconnectS3(homePath)} title="Disconnect from S3">
        &times;
      </button>
    {:else if panel.backend === 'sftp'}
      <button class="s3-disconnect" onclick={() => panel.disconnectSftp(homePath)} title="Disconnect from SFTP">
        &times;
      </button>
    {/if}
    <BreadcrumbBar
      path={panel.path}
      backend={panel.backend}
      s3Connection={panel.s3Connection}
      sftpConnection={panel.sftpConnection}
      archiveInfo={panel.archiveInfo}
      {homePath}
      onNavigate={(p) => panel.loadDirectory(p)}
      canGoBack={panel.canGoBack}
      canGoForward={panel.canGoForward}
      onGoBack={() => panel.goBack()}
      onGoForward={() => panel.goForward()}
    />
    {#if panel.gitInfo}
      <span class="backend-indicator">
        <svg class="backend-logo" viewBox="0 0 92 92" xmlns="http://www.w3.org/2000/svg">
          <path d="M90.156 41.965 50.036 1.848a5.918 5.918 0 0 0-8.372 0l-8.328 8.332 10.566 10.566a7.03 7.03 0 0 1 7.23 1.684 7.034 7.034 0 0 1 1.669 7.277l10.187 10.184a7.028 7.028 0 0 1 7.278 1.672 7.04 7.04 0 0 1 0 9.957 7.05 7.05 0 0 1-9.965 0 7.044 7.044 0 0 1-1.528-7.66l-9.5-9.497V52.68a7.072 7.072 0 0 1 1.926 1.528 7.04 7.04 0 0 1 0 9.957 7.048 7.048 0 0 1-9.965 0 7.04 7.04 0 0 1 0-9.957 7.06 7.06 0 0 1 2.307-1.681V34.8a7.06 7.06 0 0 1-2.307-1.681 7.048 7.048 0 0 1-1.516-7.608L29.242 14.961 1.73 42.471a5.918 5.918 0 0 0 0 8.372l40.121 40.12a5.916 5.916 0 0 0 8.369 0l39.937-39.934a5.92 5.92 0 0 0 0-8.373Z" fill="#F05032"/>
        </svg>
        <button class="git-branch-btn" onclick={handleBranchClick} title="Switch branch">
          {panel.gitInfo.branch.length > 20 ? panel.gitInfo.branch.slice(0, 20) + '\u2026' : panel.gitInfo.branch}
          <span class="git-branch-caret">&#x25BE;</span>
        </button>
        {#if panel.gitInfo.ahead > 0}<span class="git-ahead">&#x2191;{panel.gitInfo.ahead}</span>{/if}
        {#if panel.gitInfo.behind > 0}<span class="git-behind">&#x2193;{panel.gitInfo.behind}</span>{/if}
        {#if panel.gitInfo.dirty}<span class="git-dirty">&#x25CF;</span>{/if}
        <button class="git-pull-btn" onclick={(e) => { e.stopPropagation(); handleGitPull(); }} disabled={pulling} title="Git pull">
          {pulling ? '\u21BB' : '\u2913'}
        </button>
        {#if branchPickerOpen}
          <div class="branch-picker" bind:this={branchPickerEl}>
            {#each branchList as branch (branch)}
              <button
                class="branch-option"
                class:current={branch === panel.gitInfo?.branch}
                onclick={() => handleBranchSelect(branch)}
              >
                {branch}
                {#if branch === panel.gitInfo?.branch}<span class="branch-check">&#x2713;</span>{/if}
              </button>
            {/each}
          </div>
        {/if}
      </span>
    {:else if panel.backend === 's3'}
      <button
        class="backend-indicator backend-indicator-clickable"
        title={`Bucket properties (${platform.mod}${platform.shift}I)`}
        onclick={() => {
          if (panel.s3Connection) {
            appState.showProperties(
              `s3://${panel.s3Connection.bucket}/`,
              panel.backend,
              {
                s3ConnectionId: panel.s3Connection.connectionId,
                capabilities: panel.s3Connection.capabilities,
                s3Connection: panel.s3Connection,
              },
            );
          }
        }}
      >
        <svg class="backend-logo" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <path d="M22 12c0-1.1-.5-2.1-1.3-2.8.3-.7.4-1.4.3-2.2-.3-1.6-1.5-2.8-3.1-3.1-.8-.1-1.5 0-2.2.3C15.1 3.5 14.1 3 13 3h-2c-1.1 0-2.1.5-2.8 1.3-.7-.3-1.4-.4-2.2-.3-1.6.3-2.8 1.5-3.1 3.1-.1.8 0 1.5.3 2.2C2.5 9.9 2 10.9 2 12s.5 2.1 1.3 2.8c-.3.7-.4 1.4-.3 2.2.3 1.6 1.5 2.8 3.1 3.1.8.1 1.5 0 2.2-.3.6.7 1.6 1.2 2.7 1.2h2c1.1 0 2.1-.5 2.8-1.3.7.3 1.4.4 2.2.3 1.6-.3 2.8-1.5 3.1-3.1.1-.8 0-1.5-.3-2.2.7-.6 1.2-1.6 1.2-2.7Z" fill="none" stroke="#FF9900" stroke-width="1.5"/>
          <path d="M8 12h8M8 9h8M8 15h8" stroke="#FF9900" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        <span class="backend-label">S3</span>
        <svg class="bucket-props-icon" viewBox="0 0 16 16" width="10" height="10" fill="currentColor">
          <path d="M8 4.754a3.246 3.246 0 1 0 0 6.492 3.246 3.246 0 0 0 0-6.492ZM5.754 8a2.246 2.246 0 1 1 4.492 0 2.246 2.246 0 0 1-4.492 0Z"/>
          <path d="M9.796 1.343c-.527-1.79-3.065-1.79-3.592 0l-.094.319a.873.873 0 0 1-1.255.52l-.292-.16c-1.64-.892-3.433.902-2.54 2.541l.159.292a.873.873 0 0 1-.52 1.255l-.319.094c-1.79.527-1.79 3.065 0 3.592l.319.094a.873.873 0 0 1 .52 1.255l-.16.292c-.892 1.64.902 3.434 2.541 2.54l.292-.159a.873.873 0 0 1 1.255.52l.094.319c.527 1.79 3.065 1.79 3.592 0l.094-.319a.873.873 0 0 1 1.255-.52l.292.16c1.64.893 3.434-.902 2.54-2.541l-.159-.292a.873.873 0 0 1 .52-1.255l.319-.094c1.79-.527 1.79-3.065 0-3.592l-.319-.094a.873.873 0 0 1-.52-1.255l.16-.292c.893-1.64-.902-3.433-2.541-2.54l-.292.159a.873.873 0 0 1-1.255-.52l-.094-.319Zm-2.633.283c.246-.835 1.428-.835 1.674 0l.094.319a1.873 1.873 0 0 0 2.693 1.115l.291-.16c.764-.415 1.6.42 1.184 1.185l-.159.292a1.873 1.873 0 0 0 1.116 2.692l.318.094c.835.246.835 1.428 0 1.674l-.319.094a1.873 1.873 0 0 0-1.115 2.693l.16.291c.415.764-.42 1.6-1.185 1.184l-.291-.159a1.873 1.873 0 0 0-2.693 1.116l-.094.318c-.246.835-1.428.835-1.674 0l-.094-.319a1.873 1.873 0 0 0-2.692-1.115l-.292.16c-.764.415-1.6-.42-1.184-1.185l.159-.291a1.873 1.873 0 0 0-1.116-2.693l-.318-.094c-.835-.246-.835-1.428 0-1.674l.319-.094a1.873 1.873 0 0 0 1.115-2.692l-.16-.292c-.415-.764.42-1.6 1.185-1.184l.292.159a1.873 1.873 0 0 0 2.692-1.116l.094-.318Z"/>
        </svg>
      </button>
    {:else if panel.backend === 'sftp'}
      <button
        class="backend-indicator backend-indicator-clickable"
        title="Connection info"
        onclick={() => {
          if (panel.sftpConnection) {
            appState.showProperties(
              panel.path,
              panel.backend,
              {
                sftpConnectionId: panel.sftpConnection.connectionId,
                sftpConnection: panel.sftpConnection,
              },
            );
          }
        }}
      >
        <svg class="backend-logo" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <rect x="3" y="2" width="18" height="6" rx="1" fill="none" stroke="#6EA8FE" stroke-width="1.5"/>
          <rect x="3" y="10" width="18" height="6" rx="1" fill="none" stroke="#6EA8FE" stroke-width="1.5"/>
          <circle cx="6.5" cy="5" r="1" fill="#6EA8FE"/>
          <circle cx="6.5" cy="13" r="1" fill="#6EA8FE"/>
          <path d="M9 20h6M12 16v4" stroke="#6EA8FE" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        <span class="backend-label">SFTP</span>
      </button>
    {:else if panel.backend === 'archive'}
      <button
        class="backend-indicator backend-indicator-clickable"
        title="Archive info"
        onclick={() => {
          if (panel.archiveInfo) {
            appState.showProperties(
              panel.archiveInfo.archivePath,
              panel.backend,
              { archiveInfo: panel.archiveInfo },
            );
          }
        }}
      >
        <svg class="backend-logo" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <path d="M4 4h16v2H4zm2 4h12v12H6zm4 3v2h4v-2z" fill="#8B8BCD"/>
          <rect x="11" y="4" width="2" height="2" fill="#8B8BCD"/>
          <rect x="11" y="8" width="2" height="2" fill="#8B8BCD"/>
          <rect x="11" y="12" width="2" height="2" fill="#8B8BCD"/>
        </svg>
        <span class="backend-label">Archive</span>
      </button>
    {/if}
    <button class="view-toggle" onclick={() => panel.toggleViewMode()} title={panel.viewMode === 'list' ? 'Switch to icon view' : panel.viewMode === 'icon' ? 'Switch to column view' : 'Switch to list view'}>
      {panel.viewMode === 'list' ? '\u229E' : panel.viewMode === 'icon' ? '\u25A5' : '\u2630'}
    </button>
  </div>

  <!-- Column headers -->
  {#if panel.viewMode === 'list'}
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="column-headers" role="row" oncontextmenu={handleColumnHeaderContextMenu}>
    {#each activeColumns as col (col.id)}
      {@const sortF = isS3 && col.s3SortField ? col.s3SortField : col.sortField}
      <button class="col-header" style="flex:{col.flex};text-align:{col.textAlign}{col.id === 'size' ? ';padding-right:1ch' : ''}" onclick={() => handleColumnClick(sortF)}>
        {isS3 && col.s3Label ? col.s3Label : col.label}{sortIndicator(sortF)}
      </button>
    {/each}
  </div>
  {#if columnPickerOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="col-picker-backdrop" role="presentation" onclick={() => { columnPickerOpen = false; }}>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="col-picker" role="menu" style="left: {columnPickerX}px; top: {columnPickerY}px" onclick={(e) => e.stopPropagation()}>
        {#each ALL_COLUMNS as col (col.id)}
          <label class="col-picker-item">
            <input
              type="checkbox"
              checked={appState.visibleColumns.includes(col.id)}
              disabled={col.id === 'name'}
              onchange={() => toggleColumn(col.id)}
            />
            {col.label}
          </label>
        {/each}
      </div>
    </div>
  {/if}
  {/if}

  <!-- Filter bar -->
  {#if panel.filterText || panel.filterInput}
  <div class="filter-bar">
    <span class="filter-icon">🔍</span>
    <input
      bind:this={filterInput}
      value={panel.filterInput}
      oninput={(e) => panel.setFilterInput((e.target as HTMLInputElement).value)}
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
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  {#if panel.viewMode === 'column'}
    {#if panel.loading}
      <div class="file-list"><div class="loading-msg">Loading...</div></div>
    {:else if panel.error}
      <div class="file-list"><div class="error-msg">{panel.error}</div></div>
    {:else}
      <ColumnView
        {panel}
        {isActive}
        {side}
        {comparisonStatusMap}
        onEntryClick={(i, e) => handleRowClick(i, e)}
        onEntryDblClick={(i) => handleRowDblClick(i)}
        onEntryContextMenu={(i, e) => handleContextMenu(i, e)}
      />
    {/if}
  {:else}
  <div
    class="file-list"
    class:icon-grid={panel.viewMode === 'icon'}
    bind:this={listContainer}
    role="list"
    style={panel.viewMode === 'icon' ? `--row-height: ${ROW_HEIGHT}px; --icon-size: ${appState.iconSize}px; --grid-min: ${appState.iconSize + 32}px` : `--row-height: ${ROW_HEIGHT}px`}
    onmousedown={handleListMouseDown}
    oncontextmenu={handleEmptyContextMenu}
    onscroll={handleListScroll}
  >
    {#if panel.loading}
      <div class="loading-msg">Loading...</div>
    {:else if panel.error}
      <div class="error-msg">{panel.error}</div>
    {:else if panel.viewMode === 'icon'}
      <!-- Virtualized icon grid -->
      <div style="height: {iconTotalHeight}px; position: relative;">
        <div style="position: absolute; top: {iconOffsetY}px; left: 0; right: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(var(--grid-min, 80px), 1fr)); gap: 4px; padding: 0 8px;">
          {#each iconVisibleEntries as entry, vi (entry.path + entry.name)}
            {@const i = iconStartIndex + vi}
            <FileIcon
              {entry}
              isSelected={panel.selectedPaths.has(entry.path)}
              isCursor={i === panel.cursorIndex}
              {isActive}
              panelSide={side}
              backend={panel.backend}
              s3ConnectionId={panel.s3Connection?.connectionId}
              sftpConnectionId={panel.sftpConnection?.connectionId}
              comparisonStatus={comparisonStatusMap.get(entry.name)}
              getSelectedPaths={() => panel.getSelectedOrCurrent()}
              onclick={(e) => handleRowClick(i, e)}
              ondblclick={() => handleRowDblClick(i)}
              oncontextmenu={(e) => handleContextMenu(i, e)}
            />
          {/each}
        </div>
      </div>
    {:else}
      <!-- Virtualized list mode: sentinel div creates full scroll height -->
      <div style="height: {totalHeight}px; position: relative;">
        <div style="position: absolute; top: {offsetY}px; left: 0; right: 0;">
          {#each visibleEntries as entry, vi (entry.path + entry.name)}
            {@const i = startIndex + vi}
            <FileRow
              {entry}
              isSelected={panel.selectedPaths.has(entry.path)}
              isCursor={i === panel.cursorIndex}
              {isActive}
              rowIndex={i}
              panelSide={side}
              {isS3}
              dirSize={entry.is_dir ? panel.dirSizeCache[entry.path] : undefined}
              encrypted={panel.encryptionCache[entry.path] === true}
              backend={panel.backend}
              s3ConnectionId={panel.s3Connection?.connectionId}
              sftpConnectionId={panel.sftpConnection?.connectionId}
              comparisonStatus={comparisonStatusMap.get(entry.name)}
              getSelectedPaths={() => panel.getSelectedOrCurrent()}
              visibleColumns={appState.visibleColumns}
              onclick={(e) => handleRowClick(i, e)}
              ondblclick={() => handleRowDblClick(i)}
              oncontextmenu={(e) => handleContextMenu(i, e)}
            />
          {/each}
        </div>
      </div>
    {/if}
    {#if rubberBanding}
      <div
        class="rubber-band"
        style="
          left: {Math.min(rubberStart.x, rubberCurrent.x)}px;
          top: {Math.min(rubberStart.y, rubberCurrent.y)}px;
          width: {Math.abs(rubberCurrent.x - rubberStart.x)}px;
          height: {Math.abs(rubberCurrent.y - rubberStart.y)}px;
        "
      ></div>
    {/if}
  </div>
  {/if}

  <!-- Footer: selection info, git status, free space -->
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
    <span class="free-space">{isS3 ? `S3 ${panel.s3Connection?.bucket ?? ''}` : panel.backend === 'sftp' && panel.sftpConnection ? `SFTP ${panel.sftpConnection.host}` : `${formatSize(panel.freeSpace)} free`}</span>
  </div>

  {#if contextMenuOpen}
    <ContextMenu
      x={contextMenuX}
      y={contextMenuY}
      onClose={() => { contextMenuOpen = false; }}
      onAction={handleContextMenuAction}
      isS3={isS3}
      isFile={!!(panel.currentEntry && !panel.currentEntry.is_dir && panel.currentEntry.name !== '..')}
      isArchive={panel.backend === 'archive'}
      isLocalDir={panel.backend === 'local' && !!(panel.currentEntry && panel.currentEntry.is_dir)}
      onEmpty={contextMenuOnEmpty}
    />
  {/if}
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
    overflow: visible;
    flex: 0 0 auto;
    opacity: 0.7;
    z-index: 10;
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

  .backend-indicator {
    position: absolute;
    right: 30px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--header-text);
    opacity: 0.6;
    white-space: nowrap;
  }

  .backend-logo {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  .backend-label {
    font-weight: 500;
    font-size: 11px;
  }

  button.backend-indicator-clickable {
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    padding: 2px 4px;
    border-radius: 3px;
    transition: opacity var(--transition-fast);
  }

  button.backend-indicator-clickable:hover {
    opacity: 1;
  }

  .bucket-props-icon {
    opacity: 0.5;
    margin-left: 2px;
    transition: opacity var(--transition-fast);
  }

  button.backend-indicator-clickable:hover .bucket-props-icon {
    opacity: 1;
  }

  .git-branch-btn {
    background: none;
    border: none;
    color: var(--header-text);
    font-size: 11px;
    font-family: inherit;
    cursor: pointer;
    padding: 1px 4px;
    border-radius: 3px;
    transition: background var(--transition-fast);
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .git-branch-btn:hover {
    background: color-mix(in srgb, var(--header-text) 15%, transparent);
  }

  .git-branch-caret {
    font-size: 9px;
    opacity: 0.6;
  }

  .git-ahead {
    color: var(--success-color, #4ec990);
    font-weight: 600;
    font-size: 10px;
  }

  .git-behind {
    color: var(--warning-color, #e8a838);
    font-weight: 600;
    font-size: 10px;
  }

  .git-dirty {
    color: var(--warning-color, #e8a838);
    font-size: 8px;
    line-height: 1;
  }

  .git-pull-btn {
    background: none;
    border: none;
    color: var(--header-text);
    font-size: 11px;
    cursor: pointer;
    padding: 0 2px;
    line-height: 1;
    opacity: 0.5;
    transition: opacity var(--transition-fast);
  }

  .git-pull-btn:hover {
    opacity: 1;
    color: var(--success-color, #4ec990);
  }

  .git-pull-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .branch-picker {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    background: var(--bg-panel);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-md, 4px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    max-height: 240px;
    overflow-y: auto;
    min-width: 160px;
    z-index: 100;
    padding: 4px 0;
  }

  .branch-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    padding: 5px 12px;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .branch-option:hover {
    background: color-mix(in srgb, var(--border-active) 20%, transparent);
  }

  .branch-option.current {
    color: var(--border-active);
    font-weight: 600;
  }

  .branch-check {
    margin-left: 8px;
    font-size: 11px;
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
    min-width: 0;
  }

  .col-header:hover {
    opacity: 1;
    color: var(--border-active);
  }

  .col-picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: 90;
  }

  .col-picker {
    position: fixed;
    background: var(--dialog-bg);
    border: 1px solid var(--dialog-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-dialog);
    padding: 8px 12px;
    z-index: 91;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 120px;
  }

  .col-picker-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    padding: 2px 0;
  }

  .col-picker-item input[type="checkbox"] {
    margin: 0;
  }

  .col-picker-item input:disabled {
    opacity: 0.4;
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
    position: relative;
    flex: 1 1 0;
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    padding: 4px 0;
  }

  .rubber-band {
    position: absolute;
    border: 1px solid var(--border-active);
    background: color-mix(in srgb, var(--border-active) 15%, transparent);
    pointer-events: none;
    z-index: 10;
    border-radius: 2px;
  }

  .file-list.icon-grid {
    padding: 8px 0;
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
