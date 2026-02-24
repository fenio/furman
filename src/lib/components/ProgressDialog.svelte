<script lang="ts">
  import type { ProgressEvent } from '$lib/types';
  import { formatSize } from '$lib/utils/format.ts';

  interface Props {
    progress: ProgressEvent | null;
  }

  let { progress }: Props = $props();

  const percentage = $derived.by(() => {
    if (!progress || progress.bytes_total === 0) return 0;
    return Math.round((progress.bytes_done / progress.bytes_total) * 100);
  });

  const bytesDisplay = $derived.by(() => {
    if (!progress) return '';
    return `${formatSize(progress.bytes_done)} / ${formatSize(progress.bytes_total)}`;
  });

  const filesDisplay = $derived.by(() => {
    if (!progress) return '';
    return `File ${progress.files_done} of ${progress.files_total}`;
  });
</script>

<div class="dialog-overlay no-select" role="dialog" aria-modal="true">
  <div class="dialog-box">
    <div class="dialog-title">Progress</div>
    <div class="dialog-body">
      {#if progress}
        <div class="progress-info">
          <div class="current-file" title={progress.current_file}>
            {progress.current_file}
          </div>
          <div class="progress-bar-container">
            <div class="progress-bar-fill" style="width: {percentage}%"></div>
          </div>
          <div class="progress-stats">
            <span>{percentage}%</span>
            <span>{bytesDisplay}</span>
            <span>{filesDisplay}</span>
          </div>
        </div>
      {:else}
        <div class="progress-info">
          <div class="current-file">Preparing...</div>
          <div class="progress-bar-container">
            <div class="progress-bar-fill" style="width: 0%"></div>
          </div>
        </div>
      {/if}
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
    min-width: 50ch;
    max-width: 70ch;
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
  }

  .dialog-title {
    background: transparent;
    color: var(--dialog-title-text);
    text-align: center;
    padding: 12px 16px;
    font-weight: 600;
    font-size: 14px;
    border-bottom: 1px solid var(--dialog-border);
  }

  .dialog-body {
    padding: 20px 24px;
  }

  .progress-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .current-file {
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
  }

  .progress-bar-container {
    width: 100%;
    height: 6px;
    background: var(--progress-bar-bg);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--progress-bar-fill);
    border-radius: 3px;
    transition: width 0.1s linear;
  }

  .progress-stats {
    display: flex;
    justify-content: space-between;
    color: var(--text-secondary);
    font-size: 12px;
  }
</style>
