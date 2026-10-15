<script lang="ts">
  import type { ScanProgress, ScanOutcome } from '#lib/types/index.js';
  import { scanSummary } from '#lib/utils/scan.js';
  import { formatSize } from '#lib/utils/format.js';
  import ErrorNotice from './ErrorNotice.svelte';
  let { progress, outcome, message, showMatches = false }: {
    progress: ScanProgress; outcome?: ScanOutcome; message?: string | null; showMatches?: boolean;
  } = $props();
  let receivedAt = $state(Date.now());
  let now = $state(Date.now());
  $effect(() => { void progress; const time = Date.now(); receivedAt = time; now = time; });
  $effect(() => {
    if (outcome) return;
    const timer = setInterval(() => { now = Date.now(); }, 500);
    return () => clearInterval(timer);
  });
  const displayed = $derived({ ...progress, elapsed_ms: progress.elapsed_ms + (outcome ? 0 : Math.max(0, now - receivedAt)) });
</script>

<div class="scan-status">
  <div role="status">{scanSummary(displayed, outcome, showMatches)}</div>
  {#if progress.bytes_read > 0}
    <div>Content read: {formatSize(progress.bytes_read)} · S3 payload read: {formatSize(progress.s3_bytes_read)} (not billing totals; excludes buffering, protocol overhead and retries).</div>
  {/if}
  {#if outcome && outcome !== 'complete'}<div>Partial results cannot establish equality or authorize Sync.</div>{/if}
  {#if message}<ErrorNotice error={message} />{/if}
</div>

<style>
  .scan-status { padding: 6px 8px; font-size: 12px; line-height: 1.5; color: var(--text-secondary); overflow-wrap: anywhere; }
</style>
