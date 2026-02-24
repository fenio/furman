<script lang="ts">
  import type { FileEntry } from '$lib/types';
  import { formatSize, formatDate, formatPermissions } from '$lib/utils/format';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import ImageTooltip from './ImageTooltip.svelte';

  interface Props {
    entry: FileEntry;
    isSelected: boolean;
    isCursor: boolean;
    isActive: boolean;
    rowIndex: number;
    panelSide?: 'left' | 'right';
    onclick?: (e: MouseEvent) => void;
    ondblclick?: () => void;
  }

  let { entry, isSelected, isCursor, isActive, rowIndex, panelSide, onclick, ondblclick }: Props = $props();

  const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);
  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
  const audioExtensions = new Set(['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a']);
  const videoExtensions = new Set(['mp4', 'mkv', 'avi', 'mov', 'webm', 'wmv']);
  const codeExtensions = new Set(['js', 'ts', 'py', 'rs', 'go', 'c', 'cpp', 'h', 'java', 'rb', 'swift', 'kt', 'svelte', 'vue', 'jsx', 'tsx']);

  const icon = $derived.by(() => {
    if (entry.name === '..') return '⬆';
    if (entry.is_dir) return '📁';
    if (entry.is_symlink) return '🔗';
    const ext = (entry.extension ?? '').toLowerCase();
    if (archiveExtensions.has(ext)) return '📦';
    if (imageExtensions.has(ext)) return '🖼';
    if (audioExtensions.has(ext)) return '🎵';
    if (videoExtensions.has(ext)) return '🎬';
    if (codeExtensions.has(ext)) return '📝';
    if (ext === 'pdf') return '📕';
    if (ext === 'md' || ext === 'txt') return '📄';
    return '📄';
  });

  const displayName = $derived.by(() => {
    const n = entry.name;
    if (n === '..') return '..';
    return n;
  });

  const sizeDisplay = $derived.by(() => {
    if (entry.name === '..') return '  UP--DIR';
    if (entry.is_dir) return '   <DIR>';
    return formatSize(entry.size).padStart(8);
  });

  const dateDisplay = $derived(formatDate(entry.modified));
  const permDisplay = $derived(formatPermissions(entry.permissions));

  const gitBadgeClass = $derived.by(() => {
    switch (entry.git_status) {
      case 'M': return 'git-M';
      case 'A': return 'git-A';
      case 'D': return 'git-D';
      case '?': return 'git-Q';
      case '!': return 'git-I';
      case 'R': return 'git-R';
      case 'U': return 'git-U';
      default: return '';
    }
  });

  const rowClass = $derived.by(() => {
    let cls = 'file-row';
    if (rowIndex % 2 === 1) cls += ' alt';
    if (isCursor && isActive) cls += ' cursor-active';
    else if (isCursor) cls += ' cursor-inactive';
    if (isSelected) cls += ' selected';
    if (entry.is_dir) cls += ' directory';
    return cls;
  });

  // Image tooltip on hover
  const isImage = $derived(imageExtensions.has((entry.extension ?? '').toLowerCase()));
  let showTooltip = $state(false);
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  let rowEl: HTMLDivElement | undefined = $state(undefined);

  function onMouseEnter() {
    if (!isImage || entry.name === '..') return;
    hoverTimer = setTimeout(() => { showTooltip = true; }, 300);
  }

  function onMouseLeave() {
    if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
    showTooltip = false;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  bind:this={rowEl}
  class={rowClass}
  role="row"
  tabindex="-1"
  draggable={entry.name !== '..'}
  ondragstart={(e) => {
    if (panelSide && e.dataTransfer) {
      e.dataTransfer.setData('application/x-furman-files', JSON.stringify({ side: panelSide }));
      e.dataTransfer.effectAllowed = 'copyMove';
    }
  }}
  onmouseenter={onMouseEnter}
  onmouseleave={onMouseLeave}
  {onclick}
  {ondblclick}
>
  <span class="col-icon">{icon}</span>
  {#if entry.git_status}
    <span class="col-git {gitBadgeClass}">{entry.git_status}</span>
  {/if}
  <span class="col-name">{displayName}</span>
  <span class="col-size">{sizeDisplay}</span>
  <span class="col-date">{dateDisplay}</span>
  <span class="col-perm">{permDisplay}</span>
</div>
{#if showTooltip && rowEl}
  <ImageTooltip src={convertFileSrc(entry.path)} anchorRect={rowEl.getBoundingClientRect()} />
{/if}

<style>
  .file-row {
    display: flex;
    flex-direction: row;
    white-space: nowrap;
    height: 28px;
    line-height: 28px;
    padding: 2px 8px;
    margin: 0 4px;
    color: var(--text-primary);
    cursor: default;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .file-row:hover {
    background: var(--bg-hover);
  }

  .file-row.alt {
    background: var(--row-alt-bg);
  }

  .file-row.alt:hover {
    background: var(--bg-hover);
  }

  .file-row.directory {
    color: var(--text-dirs);
  }

  .file-row.selected {
    color: var(--selected-text);
    font-weight: 600;
  }

  .file-row.cursor-active {
    background: var(--cursor-bg);
    color: var(--cursor-text);
  }

  .file-row.cursor-active:hover {
    background: var(--cursor-bg);
  }

  .file-row.cursor-active.selected {
    background: var(--cursor-bg);
    color: var(--selected-text);
  }

  .file-row.cursor-inactive {
    background: var(--bg-surface);
    color: var(--text-secondary);
  }

  .col-icon {
    flex: 0 0 2.5ch;
    text-align: center;
    font-size: 12px;
    line-height: 28px;
    opacity: 0.7;
  }

  .col-git {
    flex: 0 0 1.8ch;
    text-align: center;
    font-size: 11px;
    font-weight: bold;
    font-family: monospace;
    line-height: 28px;
  }

  .col-git.git-M { color: var(--git-modified); }
  .col-git.git-A { color: var(--git-added); }
  .col-git.git-D { color: var(--git-deleted); }
  .col-git.git-Q { color: var(--git-untracked); }
  .col-git.git-I { color: var(--git-ignored); }
  .col-git.git-R { color: var(--git-renamed); }
  .col-git.git-U { color: var(--git-conflict); }

  .col-name {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .col-size {
    flex: 0 0 9ch;
    text-align: right;
    padding-right: 1ch;
  }

  .col-date {
    flex: 0 0 16ch;
    text-align: left;
    padding-right: 1ch;
  }

  .col-perm {
    flex: 0 0 9ch;
    text-align: left;
  }
</style>
