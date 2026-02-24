<script lang="ts">
  import { statusState } from '$lib/state/status.svelte';
  import { panels } from '$lib/state/panels.svelte';

  const isLoading = $derived(panels.left.loading || panels.right.loading);

  const displayText = $derived.by(() => {
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

  const showBar = $derived(!!displayText);
</script>

{#if showBar}
<div class="status-bar">
  {#if statusState.isProgress}
    <div class="progress-fill" style="width: {statusState.progressPercent}%"></div>
  {/if}
  <span class="status-text">
    {#if isLoading && !statusState.isProgress && !statusState.message}
      <span class="spinner">⟳</span>
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
