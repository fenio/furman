<script lang="ts">
  import { comparisonState, type ComparisonFilter } from '#lib/state/comparison.svelte.js';
  import { panels } from '#lib/state/panels.svelte.js';
  import ScanStatus from './ScanStatus.svelte';
  import ErrorNotice from './ErrorNotice.svelte';

  interface Props {
    side: 'left' | 'right';
  }

  let { side }: Props = $props();
  const counts = $derived(comparisonState.countsFor(side));
  const panel = $derived(side === 'left' ? panels.left : panels.right);
  const canDiff = $derived(!!comparisonState.diffForEntry(side, panel.path, panel.currentEntry));

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
    <span class="bar-label">{comparisonState.mode === 'sha256' ? 'Reading contents for SHA-256...' : 'Comparing metadata...'}</span>
    <button class="filter-btn" onclick={() => comparisonState.cancelScan()}>Cancel scan</button>
  {:else if comparisonState.outcome === 'complete'}
    <span class="badge green">{counts.new}</span>
    <span class="badge yellow">{counts.modified}</span>
    <span class="badge red">{counts.deleted}</span>
    <span class="bar-label">{comparisonState.mode === 'sha256' ? 'SHA-256' : 'Metadata only'}</span>
    <button class="filter-btn" disabled={!canDiff} title="Read and diff the focused modified file against its counterpart; S3 download charges may apply" onclick={() => comparisonState.showDiff(side, panel.path, panel.currentEntry)}>Show diff</button>
    <button class="filter-btn" title="Read all object contents on both sides; S3 download charges may apply" onclick={() => comparisonState.verifyContents()}>Verify SHA-256</button>
    {#each filters as f (f.id)}
      <button
        class="filter-btn"
        class:active={comparisonState.filterFor(side) === f.id}
        style="--filter-color: {f.color}"
        onclick={() => comparisonState.setFilter(f.id, side)}
      >{f.label}</button>
    {/each}
  {:else}
    <span class="bar-label">Comparison incomplete — {comparisonState.outcome ?? 'not finished'}</span>
    <button class="filter-btn" onclick={() => comparisonState.retryComparison()}>Rescan</button>
  {/if}
  <button class="close-btn" onclick={() => comparisonState.stopComparison()}>×</button>
</div>
{#if comparisonState.scanProgress}
  <ScanStatus progress={comparisonState.scanProgress} outcome={comparisonState.outcome} message={comparisonState.scanMessage} />
{:else if comparisonState.scanMessage}
  <ErrorNotice error={comparisonState.scanMessage} />
{/if}

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

  .filter-btn:disabled { opacity: 0.45; cursor: default; }

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
