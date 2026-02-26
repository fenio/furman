<script lang="ts">
  import { transfersState } from '$lib/state/transfers.svelte';
  import type { TransferStatus } from '$lib/state/transfers.svelte';
  import { formatSize, formatSpeed, formatEta } from '$lib/utils/format';
  import { s3SetBandwidthLimit } from '$lib/services/s3';
  import { appState } from '$lib/state/app.svelte';

  const visible = $derived(transfersState.panelVisible && transfersState.transfers.length > 0);
  const activeCount = $derived(transfersState.active.length);
  const queuedCount = $derived(transfersState.queued.length);
  const hasFinished = $derived(
    transfersState.transfers.some((t) => t.status === 'completed' || t.status === 'failed' || t.status === 'cancelled'),
  );

  function typeLabel(type: string): string {
    switch (type) {
      case 'copy': return 'Copy';
      case 'move': return 'Move';
      case 'extract': return 'Extract';
      default: return type;
    }
  }

  function typeLabelPast(type: string): string {
    switch (type) {
      case 'copy': return 'Copied';
      case 'move': return 'Moved';
      case 'extract': return 'Extracted';
      default: return type;
    }
  }

  function typeLabelActive(type: string): string {
    switch (type) {
      case 'copy': return 'Copying';
      case 'move': return 'Moving';
      case 'extract': return 'Extracting';
      default: return type;
    }
  }

  function transferLabel(t: { type: string; status: TransferStatus; sources: string[]; destination: string; error?: string }): string {
    const count = t.sources.length;
    const dest = t.destination.split('/').filter(Boolean).pop() ?? t.destination;
    if (t.status === 'running') {
      return `${typeLabelActive(t.type)} ${count} item(s) to ${dest}`;
    }
    if (t.status === 'completed') {
      return `${typeLabelPast(t.type)} ${count} item(s) to ${dest}`;
    }
    if (t.status === 'cancelled') {
      return `${typeLabel(t.type)} cancelled`;
    }
    if (t.status === 'failed') {
      return `${typeLabel(t.type)} failed: ${t.error ?? 'unknown error'}`;
    }
    if (t.status === 'paused') {
      return `${typeLabel(t.type)} paused — ${count} item(s) to ${dest}`;
    }
    if (t.status === 'queued') {
      return `${typeLabel(t.type)} queued — ${count} item(s) to ${dest}`;
    }
    return typeLabel(t.type);
  }

  function percentage(t: { progress: { bytes_done: number; bytes_total: number } | null }): number {
    if (!t.progress || t.progress.bytes_total === 0) return 0;
    return Math.round((t.progress.bytes_done / t.progress.bytes_total) * 100);
  }

  function handleConcurrencyChange(e: Event) {
    const val = parseInt((e.target as HTMLSelectElement).value, 10);
    if (val > 0) {
      transfersState.maxConcurrent = val;
      transfersState.processQueue();
    }
  }

  async function handleBandwidthChange(e: Event) {
    const val = parseInt((e.target as HTMLSelectElement).value, 10);
    transfersState.bandwidthLimit = val;
    await s3SetBandwidthLimit(val);
    appState.persistConfig();
  }
</script>

{#if visible}
<div class="transfer-panel">
  <div class="transfer-header">
    <span class="transfer-title">
      Transfers{#if activeCount > 0} ({activeCount} active{#if queuedCount > 0}, {queuedCount} queued{/if}){:else if queuedCount > 0} ({queuedCount} queued){/if}
    </span>
    <div class="transfer-header-buttons">
      <select class="tp-concurrency" value={transfersState.maxConcurrent} onchange={handleConcurrencyChange} title="Max concurrent transfers">
        <option value={1}>1</option>
        <option value={2}>2</option>
        <option value={3}>3</option>
        <option value={5}>5</option>
      </select>
      <select class="tp-concurrency" value={transfersState.bandwidthLimit} onchange={handleBandwidthChange} title="Bandwidth limit">
        <option value={0}>Unlimited</option>
        <option value={1048576}>1 MB/s</option>
        <option value={5242880}>5 MB/s</option>
        <option value={10485760}>10 MB/s</option>
        <option value={26214400}>25 MB/s</option>
        <option value={52428800}>50 MB/s</option>
        <option value={104857600}>100 MB/s</option>
      </select>
      {#if hasFinished}
        <button class="tp-btn" onclick={() => transfersState.dismissCompleted()}>Clear done</button>
      {/if}
      <button class="tp-btn" onclick={() => transfersState.toggle()}>Close</button>
    </div>
  </div>
  <div class="transfer-list">
    {#each transfersState.transfers as t (t.id)}
      <div
        class="transfer-item"
        class:is-failed={t.status === 'failed'}
        class:is-cancelled={t.status === 'cancelled'}
        class:is-completed={t.status === 'completed'}
        class:is-paused={t.status === 'paused'}
        class:is-queued={t.status === 'queued'}
      >
        <div class="transfer-label" title={t.sources.join(', ')}>
          {transferLabel(t)}
        </div>
        {#if t.status === 'running'}
          <div class="transfer-progress-row">
            <div class="transfer-bar-container">
              <div class="transfer-bar-fill" style="width: {percentage(t)}%"></div>
            </div>
            <span class="transfer-pct">{percentage(t)}%</span>
          </div>
          {#if t.progress}
            <div class="transfer-stats">
              <span>{formatSize(t.progress.bytes_done)} / {formatSize(t.progress.bytes_total)}{#if t.speedBytesPerSec > 0} — {formatSpeed(t.speedBytesPerSec)}{#if formatEta(t.progress.bytes_total - t.progress.bytes_done, t.speedBytesPerSec)} — {formatEta(t.progress.bytes_total - t.progress.bytes_done, t.speedBytesPerSec)} left{/if}{/if}</span>
              <span>File {t.progress.files_done} of {t.progress.files_total}</span>
            </div>
          {/if}
        {:else if t.status === 'paused'}
          <div class="transfer-progress-row">
            <div class="transfer-bar-container paused">
              <div class="transfer-bar-fill paused" style="width: {percentage(t)}%"></div>
            </div>
            <span class="transfer-pct">{percentage(t)}%</span>
          </div>
          {#if t.progress}
            <div class="transfer-stats">
              <span>{formatSize(t.progress.bytes_done)} / {formatSize(t.progress.bytes_total)}</span>
              <span>File {t.progress.files_done} of {t.progress.files_total}</span>
            </div>
          {/if}
        {:else if t.status === 'queued'}
          <div class="transfer-stats">
            <span class="queued-label">Waiting...</span>
          </div>
        {/if}
        <div class="transfer-actions">
          {#if t.status === 'running'}
            <button class="tp-btn" onclick={() => transfersState.pause(t.id)}>Pause</button>
            <button class="tp-btn tp-btn-cancel" onclick={() => transfersState.cancel(t.id)}>Cancel</button>
          {:else if t.status === 'paused'}
            <button class="tp-btn" onclick={() => transfersState.resume(t.id)}>Resume</button>
            <button class="tp-btn tp-btn-cancel" onclick={() => transfersState.cancel(t.id)}>Cancel</button>
          {:else if t.status === 'queued'}
            <button class="tp-btn" onclick={() => transfersState.moveUp(t.id)} title="Move up">&uarr;</button>
            <button class="tp-btn" onclick={() => transfersState.moveDown(t.id)} title="Move down">&darr;</button>
            <button class="tp-btn tp-btn-cancel" onclick={() => transfersState.cancel(t.id)}>Cancel</button>
          {:else}
            <button class="tp-btn" onclick={() => transfersState.dismiss(t.id)}>Dismiss</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
</div>
{/if}

<style>
  .transfer-panel {
    flex-shrink: 0;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-panel);
    max-height: 200px;
    display: flex;
    flex-direction: column;
  }

  .transfer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 28px;
    padding: 0 8px;
    background: var(--bg-header);
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .transfer-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .transfer-header-buttons {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .tp-concurrency {
    padding: 1px 4px;
    font-size: 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .tp-btn {
    padding: 2px 8px;
    font-size: 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-surface);
    color: var(--text-secondary);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .tp-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .tp-btn-cancel:hover {
    border-color: var(--text-accent);
  }

  .transfer-list {
    overflow-y: auto;
    flex: 1 1 0;
    min-height: 0;
  }

  .transfer-item {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .transfer-item:last-child {
    border-bottom: none;
  }

  .transfer-item.is-completed .transfer-label {
    color: var(--text-secondary);
  }

  .transfer-item.is-failed .transfer-label {
    color: #e06c75;
  }

  .transfer-item.is-cancelled .transfer-label {
    color: var(--text-secondary);
    font-style: italic;
  }

  .transfer-item.is-paused .transfer-label {
    color: var(--text-secondary);
  }

  .transfer-item.is-queued .transfer-label {
    color: var(--text-secondary);
    font-style: italic;
  }

  .transfer-label {
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 4px;
  }

  .transfer-progress-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 2px;
  }

  .transfer-bar-container {
    flex: 1;
    height: 4px;
    background: var(--progress-bar-bg);
    border-radius: 2px;
    overflow: hidden;
  }

  .transfer-bar-container.paused {
    background: repeating-linear-gradient(
      45deg,
      var(--progress-bar-bg),
      var(--progress-bar-bg) 4px,
      var(--border-subtle) 4px,
      var(--border-subtle) 8px
    );
  }

  .transfer-bar-fill {
    height: 100%;
    background: var(--progress-bar-fill);
    border-radius: 2px;
    transition: width 0.15s linear;
  }

  .transfer-bar-fill.paused {
    opacity: 0.6;
  }

  .transfer-pct {
    font-size: 11px;
    color: var(--text-secondary);
    min-width: 32px;
    text-align: right;
  }

  .transfer-stats {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--text-secondary);
    margin-bottom: 2px;
  }

  .queued-label {
    font-style: italic;
  }

  .transfer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
    margin-top: 2px;
  }
</style>
