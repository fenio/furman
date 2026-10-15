<script lang="ts">
  import { onMount } from 'svelte';
  import { s3ActivitySnapshot, s3ActivityClear, type S3ActivitySnapshot } from '#lib/services/s3.js';
  import { clearS3NavigationCache, s3NavigationCache, navigationCacheState } from '#lib/state/navigation-cache.svelte.js';
  import { activityExport } from '#lib/utils/activity-export.js';
  import { formatSize } from '#lib/utils/format.js';
  import ErrorNotice from './ErrorNotice.svelte';

  let { onClose }: { onClose: () => void } = $props();
  let dialog: HTMLDialogElement;
  let snapshot = $state.raw<S3ActivitySnapshot | null>(null);
  let failure = $state<unknown>(null);
  let busy = $state(false);
  let live = $state(true);
  let failedOnly = $state(false);
  let copyStatus = $state('');
  let cacheStats = $state(s3NavigationCache.stats());
  let active = true;
  let generation = 0;
  const records = $derived(snapshot?.records ?? []);
  const failures = $derived(records.filter(record => record.outcome !== 'success').length);
  const attempts = $derived(records.reduce((sum, record) => sum + record.attempts, 0));
  const retries = $derived(records.reduce((sum, record) => sum + record.retries, 0));
  const visible = $derived((failedOnly ? records.filter(record => record.outcome !== 'success') : records).slice(0, 100));

  $effect(() => { void navigationCacheState.revision; cacheStats = s3NavigationCache.stats(); });
  onMount(() => {
    dialog.showModal();
    void refresh();
    const timer = setInterval(() => { if (live) void refresh(); }, 2000);
    return () => { active = false; generation++; clearInterval(timer); };
  });

  async function refresh() {
    if (busy) return;
    const current = ++generation;
    busy = true;
    try {
      const result = await s3ActivitySnapshot();
      if (!active || current !== generation) return;
      snapshot = result; cacheStats = s3NavigationCache.stats(); failure = null;
    } catch (error) { if (active && current === generation) { failure = error; live = false; } }
    finally { if (active && current === generation) busy = false; }
  }

  async function clearActivity() {
    if (busy) return;
    busy = true; copyStatus = '';
    try { await s3ActivityClear(); snapshot = await s3ActivitySnapshot(); failure = null; }
    catch (error) { failure = error; }
    finally { busy = false; }
  }

  async function copy() {
    if (!snapshot) return;
    try { await navigator.clipboard.writeText(activityExport(snapshot)); copyStatus = 'Copied structured diagnostics'; }
    catch (error) { failure = error; copyStatus = 'Copy failed; expand the export and copy it manually.'; }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="activity-title" oncancel={(event) => { event.preventDefault(); onClose(); }}>
  <h2 id="activity-title">S3 activity and navigation cache</h2>
  <p>Session-only, last {snapshot?.capacity ?? 500} completed SDK calls. No credentials, encryption keys, URLs, bucket/object names, payloads, or raw provider messages are collected here. Review request IDs and timestamps before sharing.</p>
  <p>SDK completion does not include subsequent download-body consumption. Abandoned calls may have reached the server. Attempt/retry counts are transmit-hook counts, not billable request totals. STS, KMS, S3 Control, CloudFront, and credential-provider requests are outside this inspector.</p>
  <div class="actions">
    <label><input type="checkbox" bind:checked={live} /> Live (every 2s while open)</label>
    <label><input type="checkbox" bind:checked={failedOnly} /> Non-success only</label>
    <button onclick={refresh} disabled={busy}>Refresh activity</button>
    <button onclick={clearActivity} disabled={busy}>Clear activity</button>
  </div>
  {#if failure}<ErrorNotice error={failure} context="Activity inspector" />{/if}
  <p role="status">Retained: {records.length} calls · {records.length - failures} successful · {failures} non-success · {attempts} attempts · {retries} retries · {snapshot?.evicted ?? 0} older records evicted</p>
  <div class="table-scroll">
    <table>
      <thead><tr><th>Started</th><th>Operation</th><th>Duration</th><th>Outcome / HTTP</th><th>Attempts / retries</th><th>Request ID</th></tr></thead>
      <tbody>{#each visible as record (record.id)}<tr>
        <td>{new Date(record.started_ms).toLocaleTimeString()}</td><td>{record.operation}</td><td>{record.duration_ms} ms</td>
        <td>{record.outcome} / {record.http_status ?? '—'}</td><td>{record.attempts} / {record.retries}</td><td>{record.request_id ?? '—'}</td>
      </tr>{/each}</tbody>
    </table>
  </div>
  <p>Showing newest {visible.length} matching records; export includes all retained records, independent of this filter.</p>
  <h3>Display-only navigation cache</h3>
  <p>{cacheStats.views}/{s3NavigationCache.limits.views} views · {cacheStats.entries.toLocaleString()}/{s3NavigationCache.limits.entries.toLocaleString()} entries · estimated {formatSize(cacheStats.estimatedBytes)}/{formatSize(s3NavigationCache.limits.weight)} · {s3NavigationCache.limits.ttlMs / 1000}s TTL</p>
  <p>Complete listings only, scoped to live connections. Mutations invalidate all cached listings, including on failure. Cached rows never authorize native preflights. External changes may remain unseen until Refresh or expiry; already-open views need Refresh.</p>
  <button onclick={clearS3NavigationCache}>Clear navigation cache</button>
  <details><summary>Structured diagnostic export (no application logs)</summary>
    <p>Copy exports only the fields shown above, not connection settings or cached listings.</p>
    <pre>{snapshot ? activityExport(snapshot) : 'No snapshot yet'}</pre>
  </details>
  <footer><span role="status">{copyStatus}</span><button onclick={copy} disabled={!snapshot}>Copy diagnostics</button><button onclick={onClose}>Close</button></footer>
</dialog>

<style>
  dialog::backdrop { background: #0008; }
  dialog { background: var(--dialog-bg); color: var(--text-primary); border: 1px solid var(--dialog-border); border-radius: var(--radius-lg); padding: 20px; width: min(1100px, 92vw); max-height: 88vh; overflow: auto; font-size: 13px; }
  h2 { font-size: 16px; } h3 { font-size: 14px; } p { overflow-wrap: anywhere; } .actions, footer { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; } footer { justify-content: flex-end; margin-top: 18px; }
  .table-scroll { max-height: 35vh; overflow: auto; } table { width: 100%; border-collapse: collapse; font-size: 12px; } th, td { text-align: left; padding: 5px; border-bottom: 1px solid var(--border-subtle); overflow-wrap: anywhere; } pre { max-height: 160px; overflow: auto; white-space: pre-wrap; user-select: text; }
</style>
