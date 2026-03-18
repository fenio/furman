<script lang="ts">
  import type { PanelData } from '$lib/state/panels.svelte';
  import type { FileEntry } from '$lib/types';
  import type { ComparisonStatus } from '$lib/state/comparison.svelte';

  interface Props {
    panel: PanelData;
    isActive: boolean;
    side?: 'left' | 'right';
    comparisonStatusMap?: Map<string, ComparisonStatus>;
    onEntryClick?: (index: number, e: MouseEvent) => void;
    onEntryDblClick?: (index: number) => void;
    onEntryContextMenu?: (index: number, e: MouseEvent) => void;
  }

  import { appState } from '$lib/state/app.svelte';

  let { panel, isActive, side: _side, comparisonStatusMap, onEntryClick, onEntryDblClick, onEntryContextMenu }: Props = $props();

  let gridContainer: HTMLDivElement | undefined = $state(undefined);
  let visibleRows = $state(20);
  let containerWidth = $state(0);
  let scrollLeft = $state(0);

  const CV_ROW_HEIGHT_MAP = { compact: 18, normal: 24, comfortable: 30 } as const;
  const ROW_HEIGHT = $derived(CV_ROW_HEIGHT_MAP[appState.rowHeight]);
  const COLUMN_WIDTH = 200; // pixels per column
  const BUFFER_COLS = 1;
  const entries = $derived(panel.filteredSortedEntries);

  // Rows per column: fill visible height first, expand if entries exceed 3 columns
  const rowsPerCol = $derived(Math.max(visibleRows, Math.ceil(entries.length / 3)));

  // Total columns needed
  const totalCols = $derived(Math.ceil(entries.length / rowsPerCol));
  const totalWidth = $derived(totalCols * COLUMN_WIDTH);

  // Visible column range based on horizontal scroll
  const visibleStartCol = $derived(
    Math.max(0, Math.floor(scrollLeft / COLUMN_WIDTH) - BUFFER_COLS)
  );
  const visibleEndCol = $derived(
    Math.min(totalCols, Math.ceil((scrollLeft + containerWidth) / COLUMN_WIDTH) + BUFFER_COLS)
  );

  // Measure container height to compute visible rows
  $effect(() => {
    if (!gridContainer) return;
    const observer = new ResizeObserver(() => {
      if (gridContainer) {
        const rows = Math.max(1, Math.floor(gridContainer.clientHeight / ROW_HEIGHT));
        visibleRows = rows;
        containerWidth = gridContainer.clientWidth;
      }
    });
    observer.observe(gridContainer);
    return () => observer.disconnect();
  });

  function handleScroll(e: Event) {
    const el = e.target as HTMLElement;
    scrollLeft = el.scrollLeft;
  }

  // Scroll cursor into view
  $effect(() => {
    const idx = panel.cursorIndex;
    if (!gridContainer) return;
    const col = Math.floor(idx / rowsPerCol);
    const row = idx % rowsPerCol;

    // Horizontal scroll
    const colLeft = col * COLUMN_WIDTH;
    const colRight = colLeft + COLUMN_WIDTH;
    if (colLeft < gridContainer.scrollLeft) {
      gridContainer.scrollLeft = colLeft;
    } else if (colRight > gridContainer.scrollLeft + gridContainer.clientWidth) {
      gridContainer.scrollLeft = colRight - gridContainer.clientWidth;
    }

    // Vertical scroll
    const rowTop = row * ROW_HEIGHT;
    const rowBottom = rowTop + ROW_HEIGHT;
    if (rowTop < gridContainer.scrollTop) {
      gridContainer.scrollTop = rowTop;
    } else if (rowBottom > gridContainer.scrollTop + gridContainer.clientHeight) {
      gridContainer.scrollTop = rowBottom - gridContainer.clientHeight;
    }
  });

  // Expose rowsPerCol on panel.gridColumns so keyboard nav can use it
  $effect(() => {
    panel.gridColumns = rowsPerCol;
  });

  function getComparisonBorder(entry: FileEntry): string {
    if (!comparisonStatusMap) return '';
    const status = comparisonStatusMap.get(entry.name);
    if (!status || status === 'same') return '';
    if (status === 'new') return 'border-left: 3px solid var(--git-added)';
    if (status === 'modified') return 'border-left: 3px solid var(--git-modified)';
    if (status === 'deleted') return 'border-left: 3px solid var(--git-deleted)';
    return '';
  }

  function getIcon(entry: FileEntry): string {
    if (entry.name === '..') return '\u2B06';
    if (entry.is_dir) return '\u{1F4C1}';
    if (entry.is_symlink) return '\u{1F517}';
    const ext = (entry.extension ?? '').toLowerCase();
    const archives = ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz'];
    const images = ['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico'];
    const audio = ['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a'];
    const video = ['mp4', 'mkv', 'avi', 'mov', 'webm', 'wmv'];
    if (archives.includes(ext)) return '\u{1F4E6}';
    if (images.includes(ext)) return '\u{1F5BC}';
    if (audio.includes(ext)) return '\u{1F3B5}';
    if (video.includes(ext)) return '\u{1F3AC}';
    return '\u{1F4C4}';
  }

  /** Get visible entries for a specific column index. */
  function getColumnEntries(colIdx: number): { entry: FileEntry; globalIndex: number }[] {
    const start = colIdx * rowsPerCol;
    const end = Math.min(start + rowsPerCol, entries.length);
    const result: { entry: FileEntry; globalIndex: number }[] = [];
    for (let i = start; i < end; i++) {
      result.push({ entry: entries[i], globalIndex: i });
    }
    return result;
  }
</script>

<div
  class="column-grid"
  style="--row-h: {ROW_HEIGHT}px"
  bind:this={gridContainer}
  role="list"
  onscroll={handleScroll}
>
  <div class="column-scroll-area" style="width: {totalWidth}px; height: {rowsPerCol * ROW_HEIGHT}px; position: relative;">
    {#each { length: visibleEndCol - visibleStartCol } as _, ci}
      {@const colIdx = visibleStartCol + ci}
      {@const colEntries = getColumnEntries(colIdx)}
      <div
        class="column-slice"
        style="position: absolute; left: {colIdx * COLUMN_WIDTH}px; top: 0; width: {COLUMN_WIDTH}px; height: 100%;"
      >
        {#each colEntries as { entry, globalIndex } (entry.path + entry.name)}
          <button
            class="column-entry"
            class:is-dir={entry.is_dir}
            class:cursor-active={globalIndex === panel.cursorIndex && isActive}
            class:cursor-inactive={globalIndex === panel.cursorIndex && !isActive}
            class:selected={panel.selectedPaths.has(entry.path)}
            style="height: {ROW_HEIGHT}px; {getComparisonBorder(entry)}"
            onclick={(e) => onEntryClick?.(globalIndex, e)}
            ondblclick={() => onEntryDblClick?.(globalIndex)}
            oncontextmenu={(e) => onEntryContextMenu?.(globalIndex, e)}
          >
            <span class="entry-icon" class:dir-icon={entry.is_dir && entry.name !== '..'}>{getIcon(entry)}</span>
            <span class="entry-name">{entry.name}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>
</div>

<style>
  .column-grid {
    flex: 1 1 0;
    overflow: auto;
    min-height: 0;
    padding: 4px 0;
  }

  .column-scroll-area {
    position: relative;
  }

  .column-slice {
    display: flex;
    flex-direction: column;
  }

  .column-entry {
    display: flex;
    align-items: center;
    padding: 0 8px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    gap: 4px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    min-width: 0;
  }

  .column-entry:hover {
    background: var(--bg-hover);
  }

  .column-entry.cursor-active {
    background: var(--cursor-bg);
    color: var(--cursor-text);
  }

  .column-entry.cursor-active:hover {
    background: var(--cursor-bg);
  }

  .column-entry.cursor-active.selected {
    background: var(--cursor-bg);
    color: var(--selected-text);
  }

  .column-entry.cursor-inactive {
    background: var(--bg-surface);
    color: var(--text-secondary);
  }

  .column-entry.selected {
    color: var(--selected-text);
  }

  .column-entry.is-dir .entry-name {
    color: var(--text-dirs);
  }

  .column-entry.cursor-active.is-dir .entry-name {
    color: var(--cursor-text);
  }

  .entry-icon {
    flex: 0 0 auto;
    font-size: 12px;
    line-height: 1;
  }

  .entry-icon.dir-icon {
    filter: saturate(1.6) brightness(1.15);
  }

  .entry-name {
    flex: 1 1 0;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
