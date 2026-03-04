<script lang="ts">
  import type { FileEntry, PanelBackend } from '$lib/types';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { readFileText } from '$lib/services/tauri';
  import { formatSize, formatDate } from '$lib/utils/format';
  import { appState } from '$lib/state/app.svelte';
  import { highlightCode, detectLanguage } from '$lib/utils/highlight';

  interface Props {
    entry: FileEntry | null;
    backend: PanelBackend;
    panelPath: string;
  }

  let { entry, backend, panelPath: _panelPath }: Props = $props();

  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico']);
  const textExtensions = new Set([
    'txt', 'md', 'js', 'ts', 'py', 'rs', 'go', 'c', 'cpp', 'h', 'java', 'rb',
    'swift', 'kt', 'svelte', 'vue', 'jsx', 'tsx', 'json', 'html', 'css', 'scss',
    'less', 'xml', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'sh', 'bash',
    'zsh', 'fish', 'ps1', 'bat', 'cmd', 'makefile', 'dockerfile', 'gitignore',
    'env', 'log', 'csv', 'sql', 'graphql', 'proto', 'lock', 'editorconfig',
  ]);

  type PreviewType = 'image' | 'text' | 'pdf' | 'directory' | 'info' | 'remote' | 'none';

  const previewType = $derived.by((): PreviewType => {
    if (!entry || entry.name === '..') return 'none';
    if (backend !== 'local') return 'remote';
    if (entry.is_dir) return 'directory';
    const ext = (entry.extension ?? '').toLowerCase();
    if (imageExtensions.has(ext)) return 'image';
    if (ext === 'pdf') return 'pdf';
    if (textExtensions.has(ext) || entry.name.startsWith('.')) return 'text';
    return 'info';
  });

  let textContent = $state('');
  let highlightedContent = $state('');
  let loading = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let loadedPath = $state('');

  $effect(() => {
    const currentEntry = entry;
    const type = previewType;

    if (debounceTimer) clearTimeout(debounceTimer);

    if (type !== 'text' || !currentEntry) {
      if (loadedPath) {
        textContent = '';
        highlightedContent = '';
        loadedPath = '';
      }
      return;
    }

    if (currentEntry.path === loadedPath) return;

    loading = true;
    textContent = '';
    highlightedContent = '';
    debounceTimer = setTimeout(() => {
      readFileText(currentEntry.path).then((content) => {
        const lines = content.split('\n');
        textContent = lines.slice(0, 200).join('\n');
        if (lines.length > 200) textContent += '\n… (truncated)';
        const lang = detectLanguage(currentEntry.name);
        highlightedContent = highlightCode(textContent, lang);
        loadedPath = currentEntry.path;
      }).catch(() => {
        textContent = '(Unable to read file)';
        loadedPath = currentEntry.path;
      }).finally(() => {
        loading = false;
      });
    }, 300);
  });
</script>

<div class="preview-pane">
  {#if previewType === 'none'}
    <div class="preview-empty">No file selected</div>
  {:else if previewType === 'remote'}
    <div class="preview-empty">Preview not available for remote files</div>
  {:else if previewType === 'image' && entry}
    <div class="preview-image">
      <img src={convertFileSrc(entry.path)} alt={entry.name} />
    </div>
    <div class="preview-info-bar">{entry.name} — {formatSize(entry.size)}</div>
  {:else if previewType === 'pdf' && entry}
    <iframe class="preview-pdf" src={convertFileSrc(entry.path)} title={entry.name}></iframe>
    <div class="preview-info-bar">{entry.name} — {formatSize(entry.size)}</div>
  {:else if previewType === 'text'}
    {#if loading}
      <div class="preview-loading">Loading...</div>
    {:else}
      <pre class="preview-text hljs">{@html highlightedContent}</pre>
    {/if}
    {#if entry}
      <div class="preview-info-bar">{entry.name} — {formatSize(entry.size)}</div>
    {/if}
  {:else if previewType === 'directory' && entry}
    <div class="preview-dir">
      <div class="preview-dir-icon">📁</div>
      <div class="preview-dir-name">{entry.name}</div>
      <div class="preview-dir-meta">{formatDate(entry.modified, appState.dateFormat)}</div>
    </div>
  {:else if entry}
    <div class="preview-file-info">
      <div class="preview-file-icon">📄</div>
      <div class="preview-file-name">{entry.name}</div>
      <div class="preview-file-meta">
        <div>Size: {formatSize(entry.size)}</div>
        <div>Modified: {formatDate(entry.modified, appState.dateFormat)}</div>
        {#if entry.extension}
          <div>Type: .{entry.extension}</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .preview-pane {
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    margin: 4px;
    overflow: hidden;
  }

  .preview-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-secondary);
    font-size: 12px;
    opacity: 0.6;
    padding: 16px;
    text-align: center;
  }

  .preview-loading {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-secondary);
    font-size: 12px;
    opacity: 0.6;
  }

  .preview-image {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    padding: 8px;
    min-height: 0;
  }

  .preview-image img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: var(--radius-sm);
  }

  .preview-pdf {
    flex: 1;
    border: none;
    min-height: 0;
    background: white;
  }

  .preview-text {
    flex: 1;
    overflow: auto;
    padding: 8px;
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-all;
    min-height: 0;
    tab-size: 4;
  }

  .preview-info-bar {
    flex-shrink: 0;
    padding: 4px 8px;
    border-top: 1px solid var(--border-subtle);
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .preview-dir,
  .preview-file-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 16px;
  }

  .preview-dir-icon,
  .preview-file-icon {
    font-size: 48px;
    opacity: 0.6;
  }

  .preview-dir-name,
  .preview-file-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    word-break: break-all;
  }

  .preview-dir-meta,
  .preview-file-meta {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
  }

  .preview-file-meta div {
    line-height: 1.6;
  }
</style>
