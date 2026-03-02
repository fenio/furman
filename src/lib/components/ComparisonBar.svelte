<script lang="ts">
  import { comparisonState, type ComparisonFilter } from '$lib/state/comparison.svelte';

  const filters: { id: ComparisonFilter; label: string; color: string }[] = [
    { id: 'all', label: 'All', color: 'var(--text-secondary)' },
    { id: 'new', label: 'Only here', color: 'var(--git-added)' },
    { id: 'modified', label: 'Modified', color: 'var(--git-modified)' },
    { id: 'deleted', label: 'Only there', color: 'var(--git-deleted)' },
  ];
</script>

<div class="comparison-bar">
  {#if comparisonState.scanning}
    <span class="spinner"></span>
    <span class="bar-label">Comparing...</span>
  {:else}
    <span class="badge green">{comparisonState.counts.new}</span>
    <span class="badge yellow">{comparisonState.counts.modified}</span>
    <span class="badge red">{comparisonState.counts.deleted}</span>
    {#each filters as f}
      <button
        class="filter-btn"
        class:active={comparisonState.filter === f.id}
        style="--filter-color: {f.color}"
        onclick={() => comparisonState.setFilter(f.id)}
      >{f.label}</button>
    {/each}
  {/if}
  <button class="close-btn" onclick={() => comparisonState.stopComparison()}>×</button>
</div>

<style>
  .comparison-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 8px;
    background: var(--bg-header);
    border-bottom: 1px solid var(--border-subtle);
    font-size: 11px;
    flex-shrink: 0;
  }

  .bar-label {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border-subtle);
    border-top-color: var(--text-accent);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .badge {
    font-size: 10px;
    font-weight: 600;
    padding: 0 5px;
    border-radius: 3px;
    line-height: 18px;
    min-width: 2ch;
    text-align: center;
  }

  .badge.green {
    background: color-mix(in srgb, var(--git-added) 25%, transparent);
    color: var(--git-added);
  }

  .badge.yellow {
    background: color-mix(in srgb, var(--git-modified) 25%, transparent);
    color: var(--git-modified);
  }

  .badge.red {
    background: color-mix(in srgb, var(--git-deleted) 25%, transparent);
    color: var(--git-deleted);
  }

  .filter-btn {
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    padding: 1px 6px;
    font-size: 10px;
    font-family: inherit;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .filter-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .filter-btn.active {
    border-color: var(--filter-color);
    color: var(--filter-color);
    background: color-mix(in srgb, var(--filter-color) 10%, transparent);
  }

  .close-btn {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    padding: 0 4px;
    line-height: 1;
    font-family: inherit;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }
</style>
