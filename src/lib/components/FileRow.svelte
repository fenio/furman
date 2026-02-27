<script lang="ts">
  import type { FileEntry, PanelBackend } from '$lib/types';
  import { formatSize, formatDate, formatPermissions } from '$lib/utils/format';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import ImageTooltip from './ImageTooltip.svelte';
  import { startLocalFileDrag, startS3FileDrag, dragState } from '$lib/services/drag';
  import { statusState } from '$lib/state/status.svelte';
  import { error as logError } from '$lib/services/log';

  interface Props {
    entry: FileEntry;
    isSelected: boolean;
    isCursor: boolean;
    isActive: boolean;
    rowIndex: number;
    panelSide?: 'left' | 'right';
    isS3?: boolean;
    dirSize?: number;
    encrypted?: boolean;
    backend?: PanelBackend;
    s3ConnectionId?: string;
    getSelectedPaths?: () => string[];
    onclick?: (e: MouseEvent) => void;
    ondblclick?: () => void;
  }

  let { entry, isSelected, isCursor, isActive, rowIndex, panelSide, isS3, dirSize, encrypted, backend, s3ConnectionId, getSelectedPaths, onclick, ondblclick }: Props = $props();

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
    if (entry.is_dir) {
      if (dirSize != null) return formatSize(dirSize).padStart(8);
      return '   <DIR>';
    }
    return formatSize(entry.size).padStart(8);
  });

  const dateDisplay = $derived(formatDate(entry.modified));
  const permDisplay = $derived(formatPermissions(entry.permissions));

  const storageClassAbbrev: Record<string, string> = {
    'STANDARD': 'STD',
    'STANDARD_IA': 'STD-IA',
    'ONEZONE_IA': 'OZ-IA',
    'INTELLIGENT_TIERING': 'INT-T',
    'GLACIER': 'GLACR',
    'DEEP_ARCHIVE': 'DEEP',
    'GLACIER_IR': 'GL-IR',
    'REDUCED_REDUNDANCY': 'RR',
    'EXPRESS_ONEZONE': 'EXPR',
  };

  const lastColDisplay = $derived.by(() => {
    if (!isS3) return permDisplay;
    const sc = entry.storage_class;
    if (!sc) return entry.is_dir ? '' : 'STD';
    return storageClassAbbrev[sc] ?? sc;
  });

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
    if (!isImage || entry.name === '..' || backend !== 'local') return;
    hoverTimer = setTimeout(() => { showTooltip = true; }, 300);
  }

  function onMouseLeave() {
    if (hoverTimer) { clearTimeout(hoverTimer); hoverTimer = null; }
    showTooltip = false;
  }

  function handleDragGesture(e: MouseEvent) {
    if (entry.name === '..' || e.button !== 0) return;
    if (!panelSide || !backend || backend === 'archive') return;

    const startX = e.clientX;
    const startY = e.clientY;
    const dragSide = panelSide;
    const dragBackend = backend;
    const dragS3Id = s3ConnectionId;
    let started = false;

    function onMove(ev: MouseEvent) {
      if (started) return;
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (Math.abs(dx) + Math.abs(dy) <= 5) return;

      started = true;
      cleanup();

      const paths = getSelectedPaths ? getSelectedPaths() : [entry.path];
      if (paths.length === 0) return;

      dragState.source = { side: dragSide, backend: dragBackend, paths, s3ConnectionId: dragS3Id };

      if (dragBackend === 'local') {
        startLocalFileDrag(paths)
          .catch((err) => logError(String(err)))
          .finally(() => { dragState.source = null; });
      } else if (dragBackend === 's3' && dragS3Id) {
        statusState.setMessage('Preparing drag...');
        startS3FileDrag(dragS3Id, paths)
          .catch((err) => logError(String(err)))
          .finally(() => { dragState.source = null; statusState.setMessage(''); });
      }
    }

    function onUp() {
      cleanup();
    }

    function cleanup() {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  bind:this={rowEl}
  class={rowClass}
  role="row"
  tabindex="-1"
  onmousedown={handleDragGesture}
  onmouseenter={onMouseEnter}
  onmouseleave={onMouseLeave}
  {onclick}
  {ondblclick}
>
  <span class="col-icon">{icon}</span>
  {#if entry.git_status}
    <span class="col-git {gitBadgeClass}">{entry.git_status}</span>
  {/if}
  {#if encrypted}
    <span class="col-encrypted" title="Client-side encrypted">&#x1F512;</span>
  {/if}
  <span class="col-name">{displayName}</span>
  <span class="col-size">{sizeDisplay}</span>
  <span class="col-date">{dateDisplay}</span>
  <span class="col-perm">{lastColDisplay}</span>
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

  .col-encrypted {
    flex: 0 0 1.8ch;
    text-align: center;
    font-size: 10px;
    line-height: 28px;
    opacity: 0.7;
  }

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
