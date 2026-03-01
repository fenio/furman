<script lang="ts">
  import { transfersState } from '$lib/state/transfers.svelte';
  import type { Transfer } from '$lib/state/transfers.svelte';
  import { formatSize, formatSpeed, formatEta } from '$lib/utils/format';

  let dialogEl: HTMLDivElement | undefined = $state();

  const allTransfers = $derived(transfersState.transfers);

  const foreground = $derived(
    allTransfers.find((t) => t.status === 'running') ?? null,
  );

  const queued = $derived(
    allTransfers.filter(
      (t) => t.status === 'queued' || (t.status === 'running' && t !== foreground),
    ),
  );

  const hasAnyActive = $derived(
    allTransfers.some((t) => t.status === 'running' || t.status === 'queued'),
  );

  function percentage(t: Transfer): number {
    if (!t.progress || t.progress.bytes_total === 0) return 0;
    return Math.round((t.progress.bytes_done / t.progress.bytes_total) * 100);
  }

  function isScanning(t: Transfer): boolean {
    return t.status === 'running' && (!t.progress || t.progress.bytes_total === 0);
  }

  function typeLabel(type: string): string {
    switch (type) {
      case 'copy': return 'Copying';
      case 'move': return 'Moving';
      case 'extract': return 'Extracting';
      default: return type;
    }
  }

  function queueTypeLabel(type: string): string {
    switch (type) {
      case 'copy': return 'Copy';
      case 'move': return 'Move';
      case 'extract': return 'Extract';
      default: return type;
    }
  }

  function title(t: Transfer): string {
    const count = t.progress?.files_total || t.sources.length;
    const dest = t.destination.split('/').filter(Boolean).pop() ?? t.destination;
    return `${typeLabel(t.type)} ${count} item(s) to ${dest}`;
  }

  function handleBackground() {
    transfersState.hideDialog();
  }

  async function handleCancel() {
    for (const t of allTransfers) {
      if (t.status === 'running' || t.status === 'queued') {
        await transfersState.cancel(t.id);
      }
    }
    transfersState.hideDialog();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      handleBackground();
    }
  }

  $effect(() => {
    if (dialogEl) {
      dialogEl.focus();
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="dialog-overlay"
  role="dialog"
  aria-label="Transfer progress"
  tabindex="-1"
  onkeydown={handleKeydown}
  onclick={(e) => { if (e.target === e.currentTarget) handleBackground(); }}
>
  <div class="dialog-box" bind:this={dialogEl} tabindex="-1">
    {#if foreground}
      <div class="dialog-title">{title(foreground)}</div>

      <div class="dialog-body">
        {#if isScanning(foreground)}
          <!-- Scan phase -->
          <div class="progress-row">
            <div class="bar-container">
              <div class="bar-fill scanning"></div>
            </div>
          </div>
          <div class="scan-status">
            {#if foreground.progress?.current_file}
              {foreground.progress.current_file}
            {:else}
              Scanning...
            {/if}
          </div>
        {:else}
          <!-- Download/transfer phase -->
          <div class="progress-row">
            <div class="bar-container">
              <div class="bar-fill" style="width: {percentage(foreground)}%"></div>
            </div>
            <span class="pct">{percentage(foreground)}%</span>
          </div>

          {#if foreground.progress}
            {#if foreground.progress.current_file}
              <div class="current-file" title={foreground.progress.current_file}>
                Current: {foreground.progress.current_file.split('/').pop()}
              </div>
            {/if}
            <div class="stats-row">
              <span>
                {formatSize(foreground.progress.bytes_done)} / {formatSize(foreground.progress.bytes_total)}
                {#if foreground.speedBytesPerSec > 0}
                  &mdash; {formatSpeed(foreground.speedBytesPerSec)}
                  {#if formatEta(foreground.progress.bytes_total - foreground.progress.bytes_done, foreground.speedBytesPerSec)}
                    &mdash; {formatEta(foreground.progress.bytes_total - foreground.progress.bytes_done, foreground.speedBytesPerSec)} left
                  {/if}
                {/if}
              </span>
            </div>
            <div class="stats-row">
              <span>File {foreground.progress.files_done} of {foreground.progress.files_total}</span>
            </div>
          {/if}
        {/if}

        {#if queued.length > 0}
          <div class="queue-divider">Queue</div>
          <div class="queue-list">
            {#each queued as q (q.id)}
              <div class="queue-item">
                {queueTypeLabel(q.type)} {q.sources.length} item(s) to {q.destination.split('/').filter(Boolean).pop() ?? q.destination}
                <span class="queue-status">
                  {q.status === 'running' ? '(active)' : '(queued)'}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="dialog-title">No active transfer</div>
      <div class="dialog-body">
        <div class="scan-status">Waiting for transfer to start...</div>
      </div>
    {/if}

    <div class="dialog-footer">
      <button class="btn btn-secondary" onclick={handleBackground}>Background</button>
      <button class="btn btn-cancel" onclick={handleCancel}>Cancel</button>
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
    z-index: 200;
  }

  .dialog-box {
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    width: 520px;
    max-width: 90vw;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.4);
    outline: none;
  }

  .dialog-title {
    padding: 14px 16px 10px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border-subtle);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dialog-body {
    padding: 14px 16px;
  }

  .progress-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .bar-container {
    flex: 1;
    height: 6px;
    background: var(--progress-bar-bg);
    border-radius: 3px;
    overflow: hidden;
  }

  .bar-fill {
    height: 100%;
    background: var(--progress-bar-fill);
    border-radius: 3px;
    transition: width 0.2s linear;
  }

  .bar-fill.scanning {
    width: 30%;
    animation: scan-pulse 1.5s ease-in-out infinite;
  }

  @keyframes scan-pulse {
    0% {
      margin-left: 0%;
      width: 30%;
      opacity: 0.7;
    }
    50% {
      margin-left: 35%;
      width: 30%;
      opacity: 1;
    }
    100% {
      margin-left: 70%;
      width: 30%;
      opacity: 0.7;
    }
  }

  .pct {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    min-width: 36px;
    text-align: right;
  }

  .scan-status {
    font-size: 12px;
    color: var(--text-secondary);
    font-style: italic;
  }

  .current-file {
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: 4px;
  }

  .stats-row {
    font-size: 11px;
    color: var(--text-secondary);
    margin-bottom: 2px;
  }

  .queue-divider {
    margin-top: 14px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border-subtle);
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .queue-list {
    max-height: 80px;
    overflow-y: auto;
  }

  .queue-item {
    padding: 4px 0;
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .queue-status {
    font-style: italic;
    opacity: 0.7;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .btn {
    padding: 5px 16px;
    font-size: 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
    border: 1px solid var(--border-subtle);
  }

  .btn-secondary {
    background: var(--bg-surface);
    color: var(--text-primary);
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }

  .btn-cancel {
    background: var(--bg-surface);
    color: var(--text-primary);
  }

  .btn-cancel:hover {
    background: var(--bg-hover);
    border-color: var(--text-accent);
  }
</style>
