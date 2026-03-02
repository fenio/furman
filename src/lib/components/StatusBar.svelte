<script lang="ts">
  import { statusState } from '$lib/state/status.svelte';
  import { transfersState } from '$lib/state/transfers.svelte';
  import { operationsState } from '$lib/state/operations.svelte';
  import { panels } from '$lib/state/panels.svelte';

  const isLoading = $derived(panels.left.loading || panels.right.loading);

  const hasTransfers = $derived(transfersState.hasActive);

  const lastOp = $derived(operationsState.history[0]);
  const isNotification = $derived(operationsState.toastVisible && !!lastOp);

  const notificationMessage = $derived.by(() => {
    if (!lastOp) return '';
    switch (lastOp.type) {
      case 'delete':
        return `Deleted ${lastOp.trashItems?.length ?? lastOp.sourcePaths?.length ?? 0} file(s)`;
      case 'rename':
        return `Renamed ${lastOp.originalName} → ${lastOp.newName}`;
      case 'move':
        return `Moved ${lastOp.sourcePaths?.length ?? 0} file(s)`;
      default:
        return 'Operation completed';
    }
  });

  const canUndo = $derived(
    lastOp && !lastOp.undone && lastOp.backend === 'local'
  );

  const displayText = $derived.by(() => {
    if (isNotification) {
      return notificationMessage;
    }
    if (hasTransfers) {
      return transfersState.aggregateSummary;
    }
    if (statusState.isProgress) {
      return statusState.progressDetail || 'Working...';
    }
    if (statusState.message) {
      return statusState.message;
    }
    if (isLoading) {
      return 'Loading...';
    }
    return '';
  });

  const progressPercent = $derived(hasTransfers ? transfersState.aggregatePercent : statusState.progressPercent);
  const showProgress = $derived(!isNotification && (hasTransfers || statusState.isProgress));
  const showBar = $derived(!!displayText);
  const clickable = $derived(!isNotification && transfersState.transfers.length > 0);

  function handleClick() {
    if (clickable) {
      transfersState.toggle();
    }
  }

  function handleUndo() {
    window.dispatchEvent(new CustomEvent('undo-last-operation'));
  }
</script>

{#if showBar}
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="status-bar" class:clickable class:has-notification={isNotification} onclick={handleClick}>
  {#if showProgress}
    <div class="progress-fill" style="width: {progressPercent}%"></div>
  {/if}
  <span class="status-text">
    {#if isLoading && !showProgress && !statusState.message && !isNotification}
      <span class="spinner">&#x27F3;</span>
    {/if}
    {displayText}
  </span>
  {#if isNotification}
    <div class="notification-actions">
      {#if canUndo}
        <button class="notify-btn undo" onclick={handleUndo}>Undo</button>
      {/if}
      <button class="notify-btn dismiss" onclick={() => operationsState.dismissToast()}>×</button>
    </div>
  {/if}
</div>
{/if}

<style>
  .status-bar {
    position: relative;
    height: 24px;
    line-height: 24px;
    background: var(--bg-header);
    color: var(--text-secondary);
    text-align: center;
    font-size: 12px;
    border-top: 1px solid var(--border-subtle);
    overflow: hidden;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-bar.clickable {
    cursor: pointer;
  }

  .status-bar.clickable:hover {
    background: var(--bg-hover);
  }

  .status-bar.has-notification {
    border-left: 3px solid var(--text-accent);
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .progress-fill {
    position: absolute;
    top: 0;
    left: 0;
    bottom: 0;
    background: var(--text-accent);
    opacity: 0.15;
    transition: width 0.2s ease;
  }

  .status-text {
    position: relative;
    z-index: 1;
    flex: 1;
  }

  .notification-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-right: 8px;
    position: relative;
    z-index: 1;
  }

  .notify-btn.undo {
    background: rgba(110, 168, 254, 0.2);
    border: 1px solid var(--border-active);
    color: var(--text-accent);
    border-radius: var(--radius-sm);
    padding: 1px 8px;
    font-size: 11px;
    font-family: inherit;
    cursor: pointer;
    transition: background var(--transition-fast);
    line-height: 18px;
  }

  .notify-btn.undo:hover {
    background: rgba(110, 168, 254, 0.35);
  }

  .notify-btn.dismiss {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    font-family: inherit;
  }

  .notify-btn.dismiss:hover {
    color: var(--text-primary);
  }

  .spinner {
    display: inline-block;
    animation: spin 1s linear infinite;
    margin-right: 4px;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
