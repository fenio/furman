<script lang="ts">
  import type { PanelData } from '$lib/state/panels.svelte';
  import type { FileEntry } from '$lib/types';

  interface Props {
    panel: PanelData;
    isActive: boolean;
    side?: 'left' | 'right';
    onEntryClick?: (index: number, e: MouseEvent) => void;
    onEntryDblClick?: (index: number) => void;
  }

  let { panel, isActive, side, onEntryClick, onEntryDblClick }: Props = $props();

  let gridContainer: HTMLDivElement | undefined = $state(undefined);
  let visibleRows = $state(20);

  const ROW_HEIGHT = 24;
  const entries = $derived(panel.filteredSortedEntries);

  // Rows per column: fill visible height first, expand if entries exceed 3 columns
  const rowsPerCol = $derived(Math.max(visibleRows, Math.ceil(entries.length / 3)));

  // Measure container height to compute visible rows
  $effect(() => {
    if (!gridContainer) return;
    const observer = new ResizeObserver(() => {
      if (gridContainer) {
        const rows = Math.max(1, Math.floor(gridContainer.clientHeight / ROW_HEIGHT));
        visibleRows = rows;
      }
    });
    observer.observe(gridContainer);
    return () => observer.disconnect();
  });

  // Scroll cursor into view
  $effect(() => {
    const idx = panel.cursorIndex;
    if (!gridContainer) return;
    const el = gridContainer.querySelectorAll('.column-entry')[idx] as HTMLElement | undefined;
    el?.scrollIntoView({ block: 'nearest' });
  });

  // Expose rowsPerCol on panel.gridColumns so keyboard nav can use it
  $effect(() => {
    panel.gridColumns = rowsPerCol;
  });

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
</script>

<div
  class="column-grid"
  style="--rows: {rowsPerCol}; --row-h: {ROW_HEIGHT}px"
  bind:this={gridContainer}
  role="list"
>
  {#each entries as entry, i (entry.path + entry.name)}
    <button
      class="column-entry"
      class:is-dir={entry.is_dir}
      class:cursor-active={i === panel.cursorIndex && isActive}
      class:cursor-inactive={i === panel.cursorIndex && !isActive}
      class:selected={panel.selectedPaths.has(entry.path)}
      onclick={(e) => onEntryClick?.(i, e)}
      ondblclick={() => onEntryDblClick?.(i)}
    >
      <span class="entry-icon">{getIcon(entry)}</span>
      <span class="entry-name">{entry.name}</span>
    </button>
  {/each}
</div>

<style>
  .column-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(var(--rows), var(--row-h));
    grid-auto-flow: column;
    flex: 1 1 0;
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    padding: 4px 0;
  }

  .column-entry {
    display: flex;
    align-items: center;
    height: var(--row-h);
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

  .entry-name {
    flex: 1 1 0;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
