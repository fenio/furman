<script lang="ts">
  import type { FileEntry } from '$lib/types';
  import { convertFileSrc } from '@tauri-apps/api/core';

  interface Props {
    entry: FileEntry;
    isSelected: boolean;
    isCursor: boolean;
    isActive: boolean;
    panelSide?: 'left' | 'right';
    onclick?: (e: MouseEvent) => void;
    ondblclick?: () => void;
  }

  let { entry, isSelected, isCursor, isActive, panelSide, onclick, ondblclick }: Props = $props();

  const archiveExtensions = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz']);
  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'svg', 'webp', 'ico']);
  const audioExtensions = new Set(['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a']);
  const videoExtensions = new Set(['mp4', 'mkv', 'avi', 'mov', 'webm', 'wmv']);
  const codeExtensions = new Set(['js', 'ts', 'py', 'rs', 'go', 'c', 'cpp', 'h', 'java', 'rb', 'swift', 'kt', 'svelte', 'vue', 'jsx', 'tsx']);

  const ext = $derived((entry.extension ?? '').toLowerCase());
  const isImage = $derived(imageExtensions.has(ext));

  const icon = $derived.by(() => {
    if (entry.name === '..') return '\u2B06';
    if (entry.is_dir) return '\uD83D\uDCC1';
    if (entry.is_symlink) return '\uD83D\uDD17';
    if (archiveExtensions.has(ext)) return '\uD83D\uDCE6';
    if (isImage) return '\uD83D\uDDBC';
    if (audioExtensions.has(ext)) return '\uD83C\uDFB5';
    if (videoExtensions.has(ext)) return '\uD83C\uDFAC';
    if (codeExtensions.has(ext)) return '\uD83D\uDCDD';
    if (ext === 'pdf') return '\uD83D\uDCD5';
    if (ext === 'md' || ext === 'txt') return '\uD83D\uDCC4';
    return '\uD83D\uDCC4';
  });

  let imgFailed = $state(false);

  const tileClass = $derived.by(() => {
    let cls = 'file-tile';
    if (isCursor && isActive) cls += ' cursor-active';
    else if (isCursor) cls += ' cursor-inactive';
    if (isSelected) cls += ' selected';
    if (entry.is_dir) cls += ' directory';
    return cls;
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class={tileClass}
  role="gridcell"
  tabindex="-1"
  draggable={entry.name !== '..'}
  ondragstart={(e) => {
    if (panelSide && e.dataTransfer) {
      e.dataTransfer.setData('application/x-furman-files', JSON.stringify({ side: panelSide }));
      e.dataTransfer.effectAllowed = 'copyMove';
    }
  }}
  {onclick}
  {ondblclick}
>
  <div class="tile-thumb">
    {#if isImage && !imgFailed}
      <img
        src={convertFileSrc(entry.path)}
        alt={entry.name}
        loading="lazy"
        onerror={() => { imgFailed = true; }}
      />
    {:else}
      <span class="tile-emoji">{icon}</span>
    {/if}
  </div>
  <span class="tile-name" title={entry.name}>{entry.name}</span>
</div>

<style>
  .file-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 6px 4px 4px;
    border-radius: var(--radius-sm);
    cursor: default;
    transition: background var(--transition-fast);
    min-width: 0;
  }

  .file-tile:hover {
    background: var(--bg-hover);
  }

  .file-tile.directory {
    color: var(--text-dirs);
  }

  .file-tile.selected {
    color: var(--selected-text);
    font-weight: 600;
  }

  .file-tile.cursor-active {
    background: var(--cursor-bg);
    color: var(--cursor-text);
  }

  .file-tile.cursor-active:hover {
    background: var(--cursor-bg);
  }

  .file-tile.cursor-active.selected {
    background: var(--cursor-bg);
    color: var(--selected-text);
  }

  .file-tile.cursor-inactive {
    background: var(--bg-surface);
    color: var(--text-secondary);
  }

  .tile-thumb {
    width: var(--icon-size, 48px);
    height: var(--icon-size, 48px);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .tile-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: var(--radius-sm);
  }

  .tile-emoji {
    font-size: calc(var(--icon-size, 48px) * 0.65);
    line-height: 1;
  }

  .tile-name {
    display: block;
    width: 100%;
    text-align: center;
    font-size: 11px;
    line-height: 1.3;
    margin-top: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
