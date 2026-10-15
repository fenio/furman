<script lang="ts">
  import { onMount } from 'svelte';
  import { s3PrepareWrite, s3ExecuteWrite, s3DiscardWrite, type WritePolicy, type WritePreview } from '#lib/services/s3.js';
  import { cancelFileOperation } from '#lib/services/tauri.js';
  import { guardedWrites, type GuardedWriteRequest } from '#lib/state/guarded-writes.svelte.js';
  import { transfersState } from '#lib/state/transfers.svelte.js';
  import { statusState } from '#lib/state/status.svelte.js';
  import { panels } from '#lib/state/panels.svelte.js';
  import type { ScanProgress } from '#lib/types/index.js';
  import { formatSize } from '#lib/utils/format.js';
  import ErrorNotice from './ErrorNotice.svelte';
  import { errorText } from '#lib/utils/error-report.js';
  import { validWritePolicy, writeNeedsApproval } from '#lib/utils/guarded-writes.js';

  let { request }: { request: GuardedWriteRequest } = $props();
  let preview = $state<WritePreview | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let preparing = $state(false);
  let failure = $state<unknown>(null);
  let policy = $state<WritePolicy>('create_only');
  let approved = $state(false);
  let opId = '';
  let closed = false;
  let dialog: HTMLDialogElement;
  const needsApproval = $derived(writeNeedsApproval(policy, preview?.existing ?? 0));
  const invalidPolicy = $derived(!validWritePolicy(policy, preview));

  onMount(() => {
    dialog.showModal();
    void prepare();
    return () => { closed = true; };
  });

  async function prepare() {
    preparing = true; failure = null; approved = false;
    try {
      if (preview) await s3DiscardWrite(preview.id);
      preview = null;
      opId = 'write-preview-' + crypto.randomUUID();
      const result = await s3PrepareWrite(opId, request.sourceId, request.destinationId, request.selections, request.prefix, (event) => { progress = event; });
      if (closed) { await s3DiscardWrite(result.id); return; }
      preview = result;
    } catch (error) { failure = error; }
    finally { preparing = false; }
  }

  async function close() {
    if (preparing) {
      try { await cancelFileOperation(opId); } catch (error) { failure = error; }
      return;
    }
    try {
      if (preview) await s3DiscardWrite(preview.id);
      guardedWrites.request = null;
    } catch (error) { failure = error; }
  }

  async function execute() {
    if (!preview || invalidPolicy || (needsApproval && !approved)) return;
    const planId = preview.id;
    const id = 'guarded-write-' + crypto.randomUUID();
    guardedWrites.request = null;
    transfersState.add(id, 'copy', request.selections.map((selection) => selection.path), request.destination);
    try {
      const result = await s3ExecuteWrite(planId, id, policy, approved, request.verify, (event) => transfersState.updateProgress(id, event));
      transfersState.complete(id);
      statusState.setMessage(`Wrote ${result.written}; skipped ${result.skipped}`);
      if (result.skipped === 0) await request.onComplete?.();
      else if (request.onComplete) statusState.setMessage('Some writes were skipped; Sync deletions were not run');
    } catch (error) {
      // Failures can leave completed objects. Never automatically reuse approval.
      transfersState.fail(id, errorText(error));
    } finally {
      await Promise.all([panels.left.loadDirectory(panels.left.path), panels.right.loadDirectory(panels.right.path)]);
    }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="write-title" oncancel={(event) => { event.preventDefault(); void close(); }}>
    <h2 id="write-title">Approve S3 content writes</h2>
    <p>Destination: {request.destination}</p>
    <p>The preview checks the actual bucket, not just loaded rows. No content is written before approval. Previews expire after 15 minutes.</p>
    {#if preparing}
      <p role="status">Preparing bounded preview… {progress ? `${progress.scanned} entries; ${formatSize(progress.bytes_read)} read` : ''}</p>
    {/if}
    {#if failure}<ErrorNotice error={failure} context="Write preview failed" />{/if}
    {#if preview}
      <p>{preview.total} objects · {formatSize(preview.bytes)} · {preview.existing} already exist</p>
      {#if preview.relay}<p>S3 sources are conditionally read and staged locally, one object at a time. This needs temporary disk space and incurs download/upload costs. SSE-C keys remain connection-local.</p>{/if}
      <details><summary>Source → destination (first 50)</summary>
        <ul>{#each preview.entries as entry (entry.key)}<li>{entry.source} → {entry.key}{entry.exists ? ' (exists)' : ''}</li>{/each}</ul>
      </details>
      <label>Write policy
        <select bind:value={policy} onchange={() => { approved = false; }}>
          <option value="create_only">Create only — fail if any object exists</option>
          <option value="skip_existing">Skip existing — create missing objects only</option>
          <option value="replace_unchanged">Replace only if unchanged since preview</option>
          <option value="overwrite">Overwrite — unguarded destination writes</option>
        </select>
      </label>
      <p>Missing objects use If-None-Match: *. Conditional replacements use the preview ETag as an opaque validator, not a content hash. Unsupported conditions fail without an unguarded fallback.</p>
      {#if invalidPolicy}<p>Choose another policy: existing objects prevent create-only, and objects without ETags prevent conditional replacement.</p>{/if}
      {#if needsApproval}<label><input type="checkbox" bind:checked={approved} /> I explicitly approve replacing existing content{policy === 'overwrite' ? ', including changes made after this preview' : ' only if unchanged since this preview'}.</label>{/if}
      <p>Writes are not atomic as a group. Cancellation or failure may leave completed objects; inspect the destination and create a fresh preview before retrying. Moves, metadata edits, and legacy writes are outside this protection.</p>
    {/if}
    <footer>
      <button onclick={close}>{preparing ? 'Cancel preview' : 'Close'}</button>
      <button onclick={prepare} disabled={preparing}>Refresh preview</button>
      <button onclick={execute} disabled={preparing || invalidPolicy || (needsApproval && !approved)}>Approve and write</button>
    </footer>
</dialog>

<style>
  dialog::backdrop { background: #0008; }
  dialog { background: var(--dialog-bg); color: var(--text-primary); border: 1px solid var(--dialog-border); border-radius: var(--radius-lg); padding: 20px; width: min(740px, 90vw); max-height: 85vh; overflow: auto; font-size: 13px; }
  h2 { font-size: 16px; } p, li { overflow-wrap: anywhere; } ul { max-height: 180px; overflow: auto; } label { display: block; margin: 12px 0; } select { display: block; max-width: 100%; margin-top: 8px; } footer { display: flex; gap: 10px; justify-content: flex-end; margin-top: 18px; }
</style>
