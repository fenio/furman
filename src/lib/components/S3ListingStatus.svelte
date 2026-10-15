<script lang="ts">
  import { onMount } from 'svelte';
  import ErrorNotice from './ErrorNotice.svelte';
  import type { ListingProgress } from '#lib/utils/progressive-listing.js';

  let { progress, onCancel, onRetry, onRefresh }: {
    progress: ListingProgress;
    onCancel: () => void;
    onRetry: () => void;
    onRefresh: () => void;
  } = $props();
  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => { now = Date.now(); }, 500);
    return () => clearInterval(timer);
  });
  const seconds = $derived(Math.floor((progress.elapsedMs + (progress.status === 'loading' ? Math.max(0, now - progress.updatedAt) : 0)) / 1000));
</script>

<div class="listing-status" class:incomplete={progress.status !== 'complete'}>
  <div class="listing-summary">
    <span role="status">
      {progress.status === 'loading' ? 'Loading' : progress.status === 'complete' ? 'Complete' : 'Incomplete'}:
      {progress.items.toLocaleString()} items · {progress.pages} pages · {progress.requests} LIST calls · {seconds}s
      {#if progress.status === 'cancelled'} — cancelled{/if}
    </span>
    <div class="actions">
      {#if progress.status === 'loading'}
        <button type="button" onclick={onCancel}>Cancel</button>
      {:else if progress.status !== 'complete'}
        <button type="button" onclick={onRetry} disabled={progress.pending}>{progress.status === 'error' ? 'Retry page' : 'Continue'}</button>
      {/if}
      <button type="button" onclick={onRefresh}>Refresh</button>
    </div>
  </div>
  {#if progress.status !== 'complete'}
    <div class="scope">Sorting, filtering and Select all apply only to loaded items, not the entire prefix.</div>
    <div class="scope">Selected folders still include all their children when copied or deleted.</div>
    {#if progress.status === 'cancelled' && progress.pending}
      <div class="scope">Waiting for the current LIST call to settle; its result will be discarded.</div>
    {/if}
  {/if}
  {#if progress.error}<ErrorNotice error={progress.error} />{/if}
</div>

<style>
  .listing-status { padding: 6px 10px; border-bottom: 1px solid var(--border-subtle); font-size: 12px; color: var(--text-secondary); flex-shrink: 0; }
  .listing-summary { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 6px; }
  .actions { display: flex; gap: 6px; }
  button { font: inherit; cursor: pointer; }
  button:disabled { cursor: default; }
  .scope { margin-top: 4px; }
  .incomplete { border-left: 3px solid var(--text-secondary); }
</style>
