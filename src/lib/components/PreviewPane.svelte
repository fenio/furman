<script lang="ts">
  import type { FileEntry, PanelBackend, ModelMetadata } from '$lib/types';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { readFileText, inspectModel } from '$lib/services/tauri';
  import { formatSize, formatDate } from '$lib/utils/format';
  import { formatParams, formatVram, estimateVram, compareValues, type VramEstimate, type CompareResult } from '$lib/utils/model';
  import { appState } from '$lib/state/app.svelte';
  import { highlightCode, detectLanguage } from '$lib/utils/highlight';

  interface Props {
    entry: FileEntry | null;
    backend: PanelBackend;
    panelPath: string;
    otherEntry?: FileEntry | null;
    otherBackend?: PanelBackend;
  }

  let { entry, backend, panelPath: _panelPath, otherEntry, otherBackend }: Props = $props();

  const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico']);
  const textExtensions = new Set([
    'txt', 'md', 'js', 'ts', 'py', 'rs', 'go', 'c', 'cpp', 'h', 'java', 'rb',
    'swift', 'kt', 'svelte', 'vue', 'jsx', 'tsx', 'json', 'html', 'css', 'scss',
    'less', 'xml', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'sh', 'bash',
    'zsh', 'fish', 'ps1', 'bat', 'cmd', 'makefile', 'dockerfile', 'gitignore',
    'env', 'log', 'csv', 'sql', 'graphql', 'proto', 'lock', 'editorconfig',
  ]);
  const modelExtensions = new Set(['safetensors', 'gguf', 'onnx']);

  type PreviewType = 'image' | 'text' | 'pdf' | 'model' | 'directory' | 'info' | 'remote' | 'none';

  const previewType = $derived.by((): PreviewType => {
    if (!entry || entry.name === '..') return 'none';
    if (backend !== 'local') return 'remote';
    if (entry.is_dir) return 'directory';
    const ext = (entry.extension ?? '').toLowerCase();
    if (imageExtensions.has(ext)) return 'image';
    if (ext === 'pdf') return 'pdf';
    if (modelExtensions.has(ext)) return 'model';
    if (textExtensions.has(ext) || entry.name.startsWith('.')) return 'text';
    return 'info';
  });

  let textContent = $state('');
  let highlightedContent = $state('');
  let textError = $state('');
  let loading = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let loadedPath = $state('');

  // Model preview state
  let modelMetadata = $state<ModelMetadata | null>(null);
  let modelLoading = $state(false);
  let modelError = $state('');
  let modelLoadedPath = $state('');

  // Other model for comparison
  let otherModelMetadata = $state<ModelMetadata | null>(null);
  let otherModelLoadedPath = $state('');

  // Derived: can we compare models?
  const canCompareModels = $derived.by(() => {
    if (!entry || !otherEntry) return false;
    if (backend !== 'local' || otherBackend !== 'local') return false;
    const ext = (entry.extension ?? '').toLowerCase();
    const otherExt = (otherEntry.extension ?? '').toLowerCase();
    return modelExtensions.has(ext) && modelExtensions.has(otherExt);
  });

  const vramEstimate = $derived.by((): VramEstimate | null => {
    if (!modelMetadata) return null;
    return estimateVram(modelMetadata);
  });

  const otherVramEstimate = $derived.by((): VramEstimate | null => {
    if (!otherModelMetadata) return null;
    return estimateVram(otherModelMetadata);
  });

  // Smallest 3 fitting GPU tiers for badge display
  const fittingGpus = $derived.by(() => {
    if (!vramEstimate) return [];
    return vramEstimate.gpuFit.filter(g => g.fits).slice(0, 3);
  });

  $effect(() => {
    const currentEntry = entry;
    const type = previewType;

    if (debounceTimer) clearTimeout(debounceTimer);

    if (type !== 'text' || !currentEntry) {
      if (loadedPath) {
        textContent = '';
        highlightedContent = '';
        textError = '';
        loadedPath = '';
      }
      return;
    }

    if (currentEntry.path === loadedPath) return;

    loading = true;
    textContent = '';
    highlightedContent = '';
    textError = '';
    debounceTimer = setTimeout(() => {
      readFileText(currentEntry.path).then((content) => {
        const lines = content.split('\n');
        textContent = lines.slice(0, 200).join('\n');
        if (lines.length > 200) textContent += '\n… (truncated)';
        if (!textContent) textContent = '(Empty file)';
        const lang = detectLanguage(currentEntry.name);
        highlightedContent = highlightCode(textContent, lang);
        loadedPath = currentEntry.path;
      }).catch((err) => {
        textError = `Unable to read file: ${String(err)}`;
        loadedPath = currentEntry.path;
      }).finally(() => {
        loading = false;
      });
    }, 300);
  });

  // Model file preview loader
  $effect(() => {
    const currentEntry = entry;
    const type = previewType;

    if (type !== 'model' || !currentEntry) {
      if (modelLoadedPath) {
        modelMetadata = null;
        modelError = '';
        modelLoadedPath = '';
      }
      return;
    }

    if (currentEntry.path === modelLoadedPath) return;

    modelLoading = true;
    modelMetadata = null;
    modelError = '';
    inspectModel(currentEntry.path).then((meta) => {
      modelMetadata = meta;
      modelLoadedPath = currentEntry.path;
    }).catch((err) => {
      modelError = String(err);
      modelLoadedPath = currentEntry.path;
    }).finally(() => {
      modelLoading = false;
    });
  });

  // Load other model metadata when comparison is possible
  $effect(() => {
    if (!canCompareModels || !otherEntry) {
      if (otherModelLoadedPath) {
        otherModelMetadata = null;
        otherModelLoadedPath = '';
      }
      return;
    }

    if (otherEntry.path === otherModelLoadedPath) return;

    otherModelMetadata = null;
    inspectModel(otherEntry.path).then((meta) => {
      otherModelMetadata = meta;
      otherModelLoadedPath = otherEntry!.path;
    }).catch(() => {
      otherModelLoadedPath = otherEntry!.path;
    });
  });

  function cmpClass(result: CompareResult): string {
    if (result === 'better') return 'cmp-better';
    if (result === 'worse') return 'cmp-worse';
    return '';
  }
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
    {:else if textError}
      <div class="preview-error">{textError}</div>
    {:else}
      <pre class="preview-text hljs">{@html highlightedContent}</pre>
    {/if}
    {#if entry}
      <div class="preview-info-bar">{entry.name} — {formatSize(entry.size)}</div>
    {/if}
  {:else if previewType === 'model' && entry}
    {#if modelLoading}
      <div class="preview-loading">Loading model info...</div>
    {:else if modelError}
      <div class="preview-file-info">
        <div class="preview-file-icon">\uD83E\uDDE0</div>
        <div class="preview-file-name">{entry.name}</div>
        <div class="preview-file-meta"><div>{modelError}</div></div>
      </div>
    {:else if modelMetadata && canCompareModels && otherModelMetadata}
      <!-- Comparison view -->
      <div class="preview-comparison">
        <div class="cmp-header">Model Comparison</div>
        <table class="cmp-table">
          <thead>
            <tr>
              <th class="cmp-label-col"></th>
              <th class="cmp-val-col" title={entry.name}>{modelMetadata.model_name ?? entry.name}</th>
              <th class="cmp-val-col" title={otherEntry?.name ?? ''}>{otherModelMetadata.model_name ?? otherEntry?.name ?? ''}</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td class="cmp-label">Format</td>
              <td>{modelMetadata.format}</td>
              <td>{otherModelMetadata.format}</td>
            </tr>
            <tr>
              <td class="cmp-label">Size</td>
              <td class={cmpClass(compareValues(modelMetadata.file_size, otherModelMetadata.file_size, true))}>{formatSize(modelMetadata.file_size)}</td>
              <td class={cmpClass(compareValues(otherModelMetadata.file_size, modelMetadata.file_size, true))}>{formatSize(otherModelMetadata.file_size)}</td>
            </tr>
            <tr>
              <td class="cmp-label">Parameters</td>
              <td class={cmpClass(compareValues(modelMetadata.total_parameters, otherModelMetadata.total_parameters, false))}>{formatParams(modelMetadata.total_parameters)}</td>
              <td class={cmpClass(compareValues(otherModelMetadata.total_parameters, modelMetadata.total_parameters, false))}>{formatParams(otherModelMetadata.total_parameters)}</td>
            </tr>
            {#if modelMetadata.quantization || otherModelMetadata.quantization}
              <tr>
                <td class="cmp-label">Quantization</td>
                <td>{modelMetadata.quantization ?? '—'}</td>
                <td>{otherModelMetadata.quantization ?? '—'}</td>
              </tr>
            {/if}
            {#if vramEstimate && otherVramEstimate}
              <tr>
                <td class="cmp-label">Est. VRAM</td>
                <td class={cmpClass(compareValues(vramEstimate.total, otherVramEstimate.total, true))}>{formatVram(vramEstimate.total)}</td>
                <td class={cmpClass(compareValues(otherVramEstimate.total, vramEstimate.total, true))}>{formatVram(otherVramEstimate.total)}</td>
              </tr>
            {/if}
            <tr>
              <td class="cmp-label">Tensors</td>
              <td>{modelMetadata.tensor_count.toLocaleString()}</td>
              <td>{otherModelMetadata.tensor_count.toLocaleString()}</td>
            </tr>
            {#if modelMetadata.context_length || otherModelMetadata.context_length}
              <tr>
                <td class="cmp-label">Context</td>
                <td class={cmpClass(compareValues(modelMetadata.context_length, otherModelMetadata.context_length, false))}>{modelMetadata.context_length?.toLocaleString() ?? '—'}</td>
                <td class={cmpClass(compareValues(otherModelMetadata.context_length, modelMetadata.context_length, false))}>{otherModelMetadata.context_length?.toLocaleString() ?? '—'}</td>
              </tr>
            {/if}
            {#if modelMetadata.block_count || otherModelMetadata.block_count}
              <tr>
                <td class="cmp-label">Layers</td>
                <td>{modelMetadata.block_count ?? '—'}</td>
                <td>{otherModelMetadata.block_count ?? '—'}</td>
              </tr>
            {/if}
            {#if modelMetadata.vocab_size || otherModelMetadata.vocab_size}
              <tr>
                <td class="cmp-label">Vocab</td>
                <td>{modelMetadata.vocab_size?.toLocaleString() ?? '—'}</td>
                <td>{otherModelMetadata.vocab_size?.toLocaleString() ?? '—'}</td>
              </tr>
            {/if}
            {#if modelMetadata.architecture || otherModelMetadata.architecture}
              <tr>
                <td class="cmp-label">Architecture</td>
                <td>{modelMetadata.architecture ?? '—'}</td>
                <td>{otherModelMetadata.architecture ?? '—'}</td>
              </tr>
            {/if}
          </tbody>
        </table>
      </div>
    {:else if modelMetadata}
      <!-- Single model view -->
      <div class="preview-file-info">
        <div class="preview-model-header">
          <span class="preview-model-icon">\uD83E\uDDE0</span>
          <div class="preview-model-title">
            <div class="preview-model-name">{modelMetadata.model_name ?? entry.name}</div>
            <span class="preview-model-badge">{modelMetadata.format}</span>
          </div>
        </div>
        {#if modelMetadata.architecture}
          <div class="preview-model-arch">{modelMetadata.architecture}</div>
        {/if}
        <div class="preview-model-stats">
          <div class="stat"><span class="stat-label">Size</span><span class="stat-value">{formatSize(modelMetadata.file_size)}</span></div>
          <div class="stat"><span class="stat-label">Parameters</span><span class="stat-value">{formatParams(modelMetadata.total_parameters)}</span></div>
          <div class="stat"><span class="stat-label">Tensors</span><span class="stat-value">{modelMetadata.tensor_count}</span></div>
          {#if modelMetadata.quantization}
            <div class="stat"><span class="stat-label">Quantization</span><span class="stat-value">{modelMetadata.quantization}</span></div>
          {/if}
          {#if vramEstimate}
            <div class="stat"><span class="stat-label">Est. VRAM</span><span class="stat-value">{formatVram(vramEstimate.total)}</span></div>
          {/if}
          {#if modelMetadata.context_length}
            <div class="stat"><span class="stat-label">Context</span><span class="stat-value">{modelMetadata.context_length.toLocaleString()}</span></div>
          {/if}
          {#if modelMetadata.block_count}
            <div class="stat"><span class="stat-label">Layers</span><span class="stat-value">{modelMetadata.block_count}</span></div>
          {/if}
        </div>
        {#if fittingGpus.length > 0}
          <div class="gpu-badges">
            {#each fittingGpus as gpu (gpu.tier)}
              <span class="gpu-badge gpu-fits">{gpu.tier} GB</span>
            {/each}
          </div>
        {:else if vramEstimate}
          <div class="gpu-badges">
            <span class="gpu-badge gpu-no-fit">80+ GB</span>
          </div>
        {/if}
      </div>
    {/if}
    <div class="preview-info-bar">{entry.name} — {formatSize(entry.size)}</div>
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
    width: 100%;
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

  .preview-error {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    color: var(--text-secondary);
    font-size: 12px;
    text-align: center;
    overflow: auto;
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

  .preview-model-header {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .preview-model-icon {
    font-size: 32px;
    flex-shrink: 0;
  }

  .preview-model-title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .preview-model-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .preview-model-badge {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--accent);
    color: var(--accent-text, #fff);
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .preview-model-arch {
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
  }

  .preview-model-stats {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    width: 100%;
    max-width: 280px;
  }

  .preview-model-stats .stat {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .preview-model-stats .stat-label {
    font-size: 10px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .preview-model-stats .stat-value {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  /* GPU fit badges */
  .gpu-badges {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    justify-content: center;
  }

  .gpu-badge {
    font-size: 10px;
    padding: 2px 8px;
    border-radius: 10px;
    font-weight: 600;
  }

  .gpu-fits {
    background: color-mix(in srgb, #10b981 25%, transparent);
    color: #10b981;
  }

  .gpu-no-fit {
    background: color-mix(in srgb, #ef4444 25%, transparent);
    color: #ef4444;
  }

  /* Comparison view */
  .preview-comparison {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: auto;
    padding: 8px;
    gap: 4px;
  }

  .cmp-header {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    padding-bottom: 4px;
  }

  .cmp-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11px;
  }

  .cmp-table th {
    font-weight: 600;
    color: var(--text-primary);
    padding: 3px 6px;
    border-bottom: 1px solid var(--border-subtle);
    text-align: left;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cmp-label-col {
    width: 80px;
  }

  .cmp-val-col {
    width: 50%;
  }

  .cmp-table td {
    padding: 2px 6px;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border-subtle);
  }

  .cmp-label {
    color: var(--text-secondary);
    font-weight: 500;
    white-space: nowrap;
  }

  .cmp-better {
    color: #10b981;
    font-weight: 600;
  }

  .cmp-worse {
    color: #ef4444;
  }
</style>
