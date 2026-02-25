<script lang="ts">
  import { statusState } from '$lib/state/status.svelte';
  import { transfersState } from '$lib/state/transfers.svelte';
  import { panels } from '$lib/state/panels.svelte';

  const isLoading = $derived(panels.left.loading || panels.right.loading);

  const hasTransfers = $derived(transfersState.hasActive);

  const displayText = $derived.by(() => {
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
  const showProgress = $derived(hasTransfers || statusState.isProgress);
  const showBar = $derived(!!displayText);
  const clickable = $derived(transfersState.transfers.length > 0);

  function handleClick() {
    if (clickable) {
      transfersState.toggle();
    }
  }
</script>

{#if showBar}
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="status-bar" class:clickable onclick={handleClick}>
  {#if showProgress}
    <div class="progress-fill" style="width: {progressPercent}%"></div>
  {/if}
  <span class="status-text">
    {#if isLoading && !showProgress && !statusState.message}
      <span class="spinner">&#x27F3;</span>
    {/if}
    {displayText}
  </span>
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
  }

  .status-bar.clickable {
    cursor: pointer;
  }

  .status-bar.clickable:hover {
    background: var(--bg-hover);
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
