<script lang="ts">
  import { onMount } from 'svelte';
  import { jobJournalState } from '#lib/state/job-journal.svelte.js';
  import { loadJobHistory } from '#lib/services/job-journal.js';
  import ErrorNotice from './ErrorNotice.svelte';
  import { jobExecutionState } from '#lib/state/job-execution.svelte.js';
  let busy = $state(false);
  async function refresh() { if (busy) return; busy = true; try { await loadJobHistory(); } catch { /* visible in shared notice */ } finally { busy = false; } }
  onMount(() => { void refresh(); });
  const paused = $derived(jobJournalState.report?.storage_review_required || jobJournalState.report?.in_memory_storage_fault || Object.keys(jobJournalState.report?.pauses ?? {}).length > 0);
</script>

{#if jobJournalState.error || paused}
  <aside aria-label="Saved job journal needs attention">
    <strong>Saved jobs need local journal/review attention.</strong>
    <p>Open Sync → Saved job definitions → Local attempt history to inspect and explicitly acknowledge review. This never reconnects, retries or restores approvals. Ordinary workflows remain available; unresolved job/storage pauses block new job work. Scheduling is disabled.</p>
    {#if jobJournalState.error}<ErrorNotice error={jobJournalState.error} context="Saved-job operation or journal recovery/review" />{/if}
    <button type="button" onclick={refresh} disabled={busy}>Refresh local journal recovery</button>
  </aside>
{/if}
{#if jobExecutionState.status?.execution || jobExecutionState.error}
  <aside aria-label="Native job execution outcome">
    {#if jobExecutionState.status?.execution}
      {@const result = jobExecutionState.status.execution}
      <strong>Native job: {jobExecutionState.status.phase === 'executing' ? 'executing' : result.status}</strong>
      <p>Run {jobExecutionState.status.run_id}. Verified {result.counts.verified_files}/{result.counts.selected_files}; skipped {result.counts.skipped_files}; uncertain {result.counts.uncertain_files}. Durable outcome: {result.outcome_recorded ? 'recorded' : 'not yet confirmed'}. Cancellation is not rollback; inspect local history/destination for incomplete outcomes. No automatic retry/resume or scheduling.</p>
      {#if result.message}<ErrorNotice error={result.message} context="Job execution/outcome recording" />{/if}
    {/if}
    {#if jobExecutionState.error}<ErrorNotice error={jobExecutionState.error} context="Job execution admission/monitoring unavailable" />{/if}
  </aside>
{/if}
<style>aside { border: 1px solid var(--border-subtle); padding: 8px; font-size: 12px; } p { margin: 4px 0; } button { font: inherit; }</style>
