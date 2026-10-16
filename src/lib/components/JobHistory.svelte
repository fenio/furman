<script lang="ts">
  import { onMount } from 'svelte';
  import { loadJobHistory, reviewJobHistory, type JobHistoryReport } from '#lib/services/job-journal.js';
  import ErrorNotice from './ErrorNotice.svelte';
  let { labels = {} }: { labels?: Record<string, string> } = $props();
  let report = $state.raw<JobHistoryReport | null>(null);
  let busy = $state(false); let failure = $state<unknown>(null); let acknowledged = $state(false); let confirmClear = $state(false); let jobId = $state(''); let visible = $state(50); let active = true;
  onMount(() => { void refresh(); return () => { active = false; }; });
  async function refresh() {
    if (busy) return; busy = true; failure = null; acknowledged = false; confirmClear = false;
    try { const result = await loadJobHistory(); if (active) { report = result; jobId = ''; visible = 50; } }
    catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }
  async function review(storage: boolean) {
    if (busy || !report || report.reconciliation_deferred || !acknowledged || (!storage && !jobId)) return;
    await update(storage ? null : jobId, false);
  }
  async function clear() { if (busy || !report || report.reconciliation_deferred || hasPauses || !confirmClear) return; await update(null, true); }
  async function update(id: string | null, clearHistory: boolean) {
    if (!report) return; busy = true; failure = null;
    try { const result = await reviewJobHistory(report.revision, id, clearHistory ? confirmClear : acknowledged, clearHistory); if (active) { report = result; jobId = ''; } }
    catch (error) { if (active) failure = error; }
    finally { if (active) { busy = false; acknowledged = false; confirmClear = false; } }
  }
  const hasPauses = $derived(!!report && (report.storage_review_required || report.in_memory_storage_fault || Object.keys(report.pauses).length > 0));
</script>

<details>
  <summary>Local attempt history — never resumes or executes</summary>
  <p>Private native history stores allowlisted job/run IDs, revisions, counters, times, policy and safe outcome categories only—not object names, paths, endpoints, credentials, payloads or approval tokens. Execution attempts include selected, attempted, acknowledged and verified counts. Interrupted/uncertain writes may already exist; cancellation is not rollback. Older read-only records have no execution intent.</p>
  <p>Startup/refresh recovery is local-only. Abandoned nonterminal attempts become interrupted and pause their job for review. Nothing reconnects, scans, restores snapshots or replays approvals.</p>
  <p>Current definition names below are display labels only; native IDs/revisions identify the recorded attempt, including older or deleted definitions. Names are not stored in history.</p>
  <button type="button" onclick={refresh} disabled={busy}>Refresh history / local recovery</button>
  {#if report}
    {#if report.reconciliation_deferred}<p role="status">Recovery/review is deferred while a cooperating task/process owns the run lease or its lock is unavailable. Release/cancel the active session, wait for its work to stop, then refresh.</p>{/if}
    {#if hasPauses}
      <fieldset disabled={busy || report.reconciliation_deferred}>
        <legend>Explicit review — never retry or resume</legend>
        <label>Paused job <select bind:value={jobId} onchange={() => { acknowledged = false; }}><option value="">Choose a job pause</option>{#each Object.entries(report.pauses) as [id, pause] (id)}<option value={id}>{labels[id] ?? 'Deleted/unavailable definition'} — {id}: {pause.reason} (run {pause.run_id})</option>{/each}</select></label>
        <label><input type="checkbox" bind:checked={acknowledged} /> I reviewed the listed outcome and any storage/recovery diagnostics. I acknowledge review only, not execution; a new run needs fresh binding, comparison and preparation.</label>
        <button type="button" onclick={() => review(false)} disabled={!acknowledged || !jobId}>Acknowledge selected job review</button>
        {#if report.storage_review_required || report.in_memory_storage_fault}<button type="button" onclick={() => review(true)} disabled={!acknowledged}>Acknowledge journal storage recovery review</button>{/if}
      </fieldset>
    {/if}
    <p>{report.records.length} retained attempts (maximum 500 globally / 50 per job). A preview discarded/cancelled/no-changes outcome is not a completed transfer. Skipped files are not called synced. Interrupted records never resume.</p>
    <div class="history"><table><thead><tr><th>Job / revision</th><th>Run</th><th>Outcome / stage</th><th>Updated (local display)</th><th>Read-stage calls / S3 bytes</th><th>Attempted / acknowledged / verified / uncertain; skipped</th></tr></thead><tbody>
      {#each report.records.slice(0, visible) as record (record.run_id)}<tr><td>{labels[record.job_id] ?? 'Deleted/unavailable definition'}<br />{record.job_id} / {record.job_revision}</td><td>{record.run_id}</td><td>{record.status}{record.policy ? ` (${record.policy})` : ''}</td><td>{new Date(record.updated_ms).toLocaleString()}</td><td>{record.binding.requests + record.comparison.requests + record.preparation.requests + (record.review?.requests ?? 0)} / {record.binding.s3_bytes_read + record.comparison.s3_bytes_read + record.preparation.s3_bytes_read + (record.review?.s3_bytes_read ?? 0)}</td><td>{#if record.execution}{record.execution.attempted_files} / {record.execution.acknowledged_files} / {record.execution.verified_files} / {record.execution.uncertain_files}; skipped {record.execution.skipped_files}{:else}Read-only attempt{/if}</td></tr>{/each}
    </tbody></table></div>
    {#if report.records.length > visible}<button type="button" onclick={() => { visible += 50; }}>Show 50 more local records</button>{/if}
    <label><input type="checkbox" bind:checked={confirmClear} disabled={busy || hasPauses || report.reconciliation_deferred} /> Confirm removal of resolved terminal history only (not definitions); unresolved pauses/active work cannot be hidden.</label>
    <button type="button" onclick={clear} disabled={busy || !confirmClear || hasPauses || report.reconciliation_deferred}>Remove resolved terminal history</button>
  {/if}
  {#if failure}<ErrorNotice error={failure} context="Local saved-job history" />{/if}
</details>
<style>details { border: 1px solid var(--border-subtle); margin: 8px 0; padding: 8px; font-size: 12px; } p { line-height: 1.5; overflow-wrap: anywhere; } label { display: block; margin: 8px 0; } .history { max-height: 240px; overflow: auto; } table { width: 100%; } th { text-align: left; } td { overflow-wrap: anywhere; } button { font: inherit; margin: 4px; } fieldset { min-width: 0; }</style>
