<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { s3BatchPutObjectMetadata, s3BatchPutObjectTags } from '$lib/services/s3';
  import { cancelFileOperation } from '$lib/services/tauri';
  import type { ProgressEvent, S3ProviderCapabilities, S3Tag } from '$lib/types';

  interface Props {
    keys: string[];
    s3ConnectionId: string;
    capabilities?: S3ProviderCapabilities;
    onClose: () => void;
  }

  let { keys, s3ConnectionId, capabilities, onClose }: Props = $props();

  const caps: S3ProviderCapabilities = untrack(() => capabilities) ?? {
    versioning: true, lifecycleRules: true, cors: true, bucketPolicy: true,
    acl: true, publicAccessBlock: true, encryption: true,
    storageClasses: ['STANDARD', 'STANDARD_IA', 'ONEZONE_IA', 'INTELLIGENT_TIERING', 'GLACIER', 'DEEP_ARCHIVE', 'GLACIER_IR'],
    glacierRestore: true, presignedUrls: true, objectMetadata: true,
    objectTags: true, bucketTags: true, multipartUploadCleanup: true,
    websiteHosting: true, requesterPays: true, objectOwnership: true, serverAccessLogging: true,
    objectLock: true,
    listBuckets: true,
  };

  type TabType = 'metadata' | 'tags';
  let activeTab = $state<TabType>(caps.objectMetadata ? 'metadata' : 'tags');

  // Metadata fields
  let contentType = $state('');
  let contentDisposition = $state('');
  let cacheControl = $state('');
  let contentEncoding = $state('');
  let customMeta = $state<Array<{ key: string; value: string }>>([]);

  // Tags fields
  let tags = $state<Array<{ key: string; value: string }>>([{ key: '', value: '' }]);
  let mergeMode = $state(false);

  // Operation state
  type Phase = 'edit' | 'progress' | 'done';
  let phase = $state<Phase>('edit');
  let opId = $state('');
  let filesDone = $state(0);
  let filesTotal = $state(0);
  let currentFile = $state('');
  let failedKeys = $state<string[]>([]);
  let showFailedList = $state(false);

  let overlayEl: HTMLDivElement;

  onMount(() => {
    overlayEl?.focus();
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (phase === 'progress') {
        handleCancel();
      } else {
        onClose();
      }
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    if (e.target === overlayEl && phase !== 'progress') {
      onClose();
    }
  }

  function addCustomMeta() {
    customMeta = [...customMeta, { key: '', value: '' }];
  }

  function removeCustomMeta(i: number) {
    customMeta = customMeta.filter((_, idx) => idx !== i);
  }

  function addTag() {
    if (tags.length < 10) {
      tags = [...tags, { key: '', value: '' }];
    }
  }

  function removeTag(i: number) {
    tags = tags.filter((_, idx) => idx !== i);
  }

  function onProgress(evt: ProgressEvent) {
    filesDone = evt.files_done;
    filesTotal = evt.files_total;
    currentFile = evt.current_file;
  }

  function handleCancel() {
    if (opId) {
      cancelFileOperation(opId).catch(() => {});
    }
  }

  function generateOpId(): string {
    return `batch-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  }

  async function applyMetadata() {
    const id = generateOpId();
    opId = id;
    phase = 'progress';
    filesDone = 0;
    filesTotal = keys.length;

    const custom: Record<string, string> = {};
    for (const m of customMeta) {
      if (m.key.trim()) {
        custom[m.key.trim()] = m.value;
      }
    }

    try {
      const failed = await s3BatchPutObjectMetadata(
        s3ConnectionId,
        id,
        keys,
        contentType || null,
        contentDisposition || null,
        cacheControl || null,
        contentEncoding || null,
        custom,
        onProgress,
      );
      failedKeys = failed;
    } catch (err) {
      failedKeys = [`Error: ${err}`];
    }

    phase = 'done';
  }

  async function applyTags() {
    const validTags: S3Tag[] = tags
      .filter(t => t.key.trim())
      .map(t => ({ key: t.key.trim(), value: t.value }));

    if (validTags.length === 0) return;

    const id = generateOpId();
    opId = id;
    phase = 'progress';
    filesDone = 0;
    filesTotal = keys.length;

    try {
      const failed = await s3BatchPutObjectTags(
        s3ConnectionId,
        id,
        keys,
        validTags,
        mergeMode,
        onProgress,
      );
      failedKeys = failed;
    } catch (err) {
      failedKeys = [`Error: ${err}`];
    }

    phase = 'done';
  }

  function shortKey(key: string): string {
    const parts = key.split('/');
    return parts[parts.length - 1] || key;
  }

  let progressPercent = $derived(filesTotal > 0 ? Math.round((filesDone / filesTotal) * 100) : 0);
  let succeeded = $derived(filesTotal - failedKeys.length);

  let hasMetadataValues = $derived(
    contentType.trim() !== '' ||
    contentDisposition.trim() !== '' ||
    cacheControl.trim() !== '' ||
    contentEncoding.trim() !== '' ||
    customMeta.some(m => m.key.trim() !== '')
  );

  let hasTagValues = $derived(tags.some(t => t.key.trim() !== ''));
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="dialog-overlay no-select"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  bind:this={overlayEl}
  onkeydown={handleKeydown}
  onclick={handleOverlayClick}
>
  <div class="dialog-box">
    <div class="dialog-title">Batch Edit &mdash; {keys.length} objects</div>
    <div class="dialog-body">
      {#if phase === 'edit'}
        <div class="tab-bar">
          {#if caps.objectMetadata}
            <button class="tab-btn" class:active={activeTab === 'metadata'} onclick={() => { activeTab = 'metadata'; }}>Metadata</button>
          {/if}
          {#if caps.objectTags}
            <button class="tab-btn" class:active={activeTab === 'tags'} onclick={() => { activeTab = 'tags'; }}>Tags</button>
          {/if}
        </div>

        {#if activeTab === 'metadata' && caps.objectMetadata}
          <p class="hint">Only non-empty fields will be applied. Existing metadata on each object will be replaced.</p>
          <div class="tag-editor">
            <label class="meta-field">
              <span class="meta-label">Content-Type</span>
              <input class="meta-input" type="text" bind:value={contentType} placeholder="e.g. image/png" />
            </label>
            <label class="meta-field">
              <span class="meta-label">Content-Disposition</span>
              <input class="meta-input" type="text" bind:value={contentDisposition} placeholder="e.g. attachment" />
            </label>
            <label class="meta-field">
              <span class="meta-label">Cache-Control</span>
              <input class="meta-input" type="text" bind:value={cacheControl} placeholder="e.g. max-age=3600" />
            </label>
            <label class="meta-field">
              <span class="meta-label">Content-Encoding</span>
              <input class="meta-input" type="text" bind:value={contentEncoding} placeholder="e.g. gzip" />
            </label>
            <div class="tag-header">
              <span class="meta-label">Custom Metadata</span>
              <button class="version-action-btn" onclick={addCustomMeta}>+ Add</button>
            </div>
            {#each customMeta as meta, i}
              <div class="tag-row">
                <input class="tag-input" type="text" bind:value={meta.key} placeholder="key" />
                <input class="tag-input" type="text" bind:value={meta.value} placeholder="value" />
                <button class="version-action-btn danger" onclick={() => removeCustomMeta(i)} title="Remove">&times;</button>
              </div>
            {/each}
            <div class="tag-actions">
              <button class="dialog-btn apply-btn" onclick={applyMetadata} disabled={!hasMetadataValues}>
                Apply Metadata
              </button>
            </div>
          </div>
        {/if}

        {#if activeTab === 'tags' && caps.objectTags}
          <div class="mode-selector">
            <label class="radio-label">
              <input type="radio" name="tag-mode" value="replace" checked={!mergeMode} onchange={() => { mergeMode = false; }} />
              Replace all tags
            </label>
            <label class="radio-label">
              <input type="radio" name="tag-mode" value="merge" checked={mergeMode} onchange={() => { mergeMode = true; }} />
              Merge with existing
            </label>
          </div>
          <div class="tag-editor">
            {#each tags as tag, i}
              <div class="tag-row">
                <input class="tag-input" type="text" bind:value={tag.key} placeholder="key" />
                <input class="tag-input" type="text" bind:value={tag.value} placeholder="value" />
                <button class="version-action-btn danger" onclick={() => removeTag(i)} title="Remove">&times;</button>
              </div>
            {/each}
            <div class="tag-actions">
              <button class="version-action-btn" onclick={addTag} disabled={tags.length >= 10}>+ Add Tag</button>
              <button class="dialog-btn apply-btn" onclick={applyTags} disabled={!hasTagValues}>
                Apply Tags
              </button>
            </div>
            {#if tags.length >= 10}
              <div class="max-tags-hint">Maximum 10 tags per object</div>
            {/if}
          </div>
        {/if}

      {:else if phase === 'progress'}
        <div class="progress-section">
          <div class="progress-info">{filesDone} / {filesTotal} objects</div>
          <div class="progress-bar-track">
            <div class="progress-bar-fill" style="width: {progressPercent}%"></div>
          </div>
          {#if currentFile}
            <div class="progress-current">{shortKey(currentFile)}</div>
          {/if}
          <div class="tag-actions">
            <button class="dialog-btn" onclick={handleCancel}>Cancel</button>
          </div>
        </div>

      {:else if phase === 'done'}
        <div class="results-section">
          <div class="results-summary">
            <span class="results-ok">{succeeded} succeeded</span>
            {#if failedKeys.length > 0}
              <span class="results-fail">{failedKeys.length} failed</span>
            {/if}
          </div>
          {#if failedKeys.length > 0}
            <button class="version-action-btn" onclick={() => { showFailedList = !showFailedList; }}>
              {showFailedList ? 'Hide' : 'Show'} failed keys
            </button>
            {#if showFailedList}
              <div class="failed-list">
                {#each failedKeys as key}
                  <div class="failed-key">{key}</div>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
    <div class="dialog-footer">
      <button class="dialog-btn" onclick={onClose} disabled={phase === 'progress'}>
        {phase === 'done' ? 'Close' : 'Cancel'}
      </button>
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
    width: 56ch;
    max-width: 90vw;
    max-height: 85vh;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .dialog-body {
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    user-select: text;
    -webkit-user-select: text;
  }

  .dialog-footer {
    display: flex;
    justify-content: center;
    padding: 16px 24px;
    border-top: 1px solid var(--dialog-border);
    flex-shrink: 0;
  }

  .dialog-btn {
    padding: 8px 24px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    font-family: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .dialog-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }

  .dialog-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: 4px;
  }

  .tab-btn {
    padding: 6px 16px;
    font-size: 12px;
    font-family: inherit;
    font-weight: 500;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-btn.active {
    border-bottom: 2px solid var(--text-accent);
    color: var(--text-accent);
  }

  .hint {
    font-size: 11px;
    color: var(--text-secondary);
    margin: 0;
  }

  .tag-editor {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 0;
  }

  .tag-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .tag-input {
    flex: 1;
    padding: 3px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
  }

  .tag-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .tag-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0 2px;
  }

  .tag-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 4px;
  }

  .meta-field {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .meta-label {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    min-width: 130px;
  }

  .meta-input {
    flex: 1;
    padding: 3px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
  }

  .meta-input:focus {
    outline: none;
    border-color: var(--border-active);
  }

  .version-action-btn {
    padding: 2px 6px;
    font-size: 10px;
    font-family: inherit;
    border: 1px solid var(--border-subtle);
    border-radius: 2px;
    background: var(--bg-surface);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .version-action-btn:hover {
    background: var(--bg-hover);
    border-color: var(--border-active);
  }

  .version-action-btn.danger:hover {
    border-color: var(--text-error, #ff6b6b);
    color: var(--text-error, #ff6b6b);
  }

  .version-action-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .apply-btn {
    padding: 6px 18px;
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    border-radius: var(--radius-sm);
    color: var(--text-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .apply-btn:hover {
    background: rgba(110, 168, 254, 0.3);
  }

  .apply-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .mode-selector {
    display: flex;
    gap: 16px;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
  }

  .max-tags-hint {
    font-size: 11px;
    color: var(--text-secondary);
    text-align: center;
    padding: 4px;
  }

  /* Progress */
  .progress-section {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 20px 0;
  }

  .progress-info {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .progress-bar-track {
    width: 100%;
    height: 6px;
    background: var(--border-subtle);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--text-accent);
    border-radius: 3px;
    transition: width 0.2s ease;
  }

  .progress-current {
    font-size: 11px;
    color: var(--text-secondary);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Results */
  .results-section {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 20px 0;
  }

  .results-summary {
    display: flex;
    gap: 16px;
    font-size: 14px;
    font-weight: 500;
  }

  .results-ok {
    color: var(--success-color, #4ec990);
  }

  .results-fail {
    color: var(--text-error, #ff6b6b);
  }

  .failed-list {
    width: 100%;
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 4px;
  }

  .failed-key {
    font-size: 11px;
    color: var(--text-secondary);
    padding: 2px 4px;
    word-break: break-all;
  }
</style>
