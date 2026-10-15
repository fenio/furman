<script lang="ts">
  import { errorReport } from '#lib/utils/error-report.js';

  let { error, context }: { error: unknown; context?: string } = $props();
  const report = $derived(errorReport(error, context));
  let copyStatus = $state('');

  $effect(() => { void report.copyText; copyStatus = ''; });

  async function copy() {
    try {
      await navigator.clipboard.writeText(report.copyText);
      copyStatus = 'Copied';
    } catch {
      copyStatus = 'Could not copy. Select the details to copy them manually.';
    }
  }
</script>

<div class="error-notice">
  <div class="error-summary" role="alert">{report.summary}</div>
  {#if report.details}
    {#key report.copyText}
      <details>
        <summary>Details</summary>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable diagnostics need keyboard focus.) -->
        <pre tabindex="0" role="region" aria-label="Error details">{report.details}</pre>
        <button type="button" onclick={copy}>Copy error</button>
        <span role="status">{copyStatus}</span>
      </details>
    {/key}
  {/if}
</div>

<style>
  .error-notice { min-width: 0; color: var(--error-color); font-size: inherit; user-select: text; }
  .error-summary { white-space: pre-wrap; overflow-wrap: anywhere; }
  details { margin-top: 6px; color: var(--text-secondary); }
  summary { cursor: pointer; width: fit-content; }
  pre { max-height: min(240px, 35vh); overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; padding: 8px; border: 1px solid var(--border-subtle); background: var(--bg-panel); }
  button { font: inherit; cursor: pointer; }
  [role='status'] { margin-left: 8px; }
</style>
