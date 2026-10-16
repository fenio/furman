<script lang="ts">
  import { onMount } from 'svelte';
  import { jobJournalState } from '#lib/state/job-journal.svelte.js';
  import { loadJobHistory } from '#lib/services/job-journal.js';
  import ErrorNotice from './ErrorNotice.svelte';
  let busy = $state(false);
  async function refresh() { if (busy) return; busy = true; try { await loadJobHistory(); } catch { /* visible in shared notice */ } finally { busy = false; } }
  onMount(() => { void refresh(); });
  const paused = $derived(jobJournalState.report?.storage_review_required || jobJournalState.report?.in_memory_storage_fault || Object.keys(jobJournalState.report?.pauses ?? {}).length > 0);
</script>

{#if jobJournalState.error || paused}
  <aside aria-label="Saved job journal needs attention">
    <strong>Saved jobs need local journal/review attention.</strong>
    <p>Open Sync → Saved job definitions → Local attempt history to inspect and explicitly acknowledge review. This never reconnects, retries or restores approvals. Ordinary workflows remain available; job execution and scheduling are disabled.</p>
    {#if jobJournalState.error}<ErrorNotice error={jobJournalState.error} context="Saved-job operation or journal recovery/review" />{/if}
    <button type="button" onclick={refresh} disabled={busy}>Refresh local journal recovery</button>
  </aside>
{/if}
<style>aside { border: 1px solid var(--border-subtle); padding: 8px; font-size: 12px; } p { margin: 4px 0; } button { font: inherit; }</style>
