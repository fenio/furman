<script lang="ts">
  import ErrorNotice from './ErrorNotice.svelte';
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { textDiff, type DiffPair, type DiffLine } from '#lib/utils/file-diff.js';
  let { pair, onClose }: { pair: DiffPair; onClose: () => void } = $props();
  let dialog: HTMLDivElement;
  let loading = $state(true);
  let error = $state('');
  let binary = $state(false);
  let rows = $state<DiffLine[]>([]);
  let disposed = false;
  interface Content { text: string | null; bytes: number }
  onMount(() => {
    dialog.focus();
    let timer: ReturnType<typeof setTimeout>;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error('Reading files timed out. Check the connections and try again.')), 30000);
    });
    const reads = Promise.all([pair.left, pair.right].map(source => invoke<Content>('comparison_read_file', { source })));
    Promise.race([reads, timeout]).then(([left, right]) => {
      if (disposed) return;
      binary = left.text === null || right.text === null;
      if (!binary) rows = textDiff(left.text!, right.text!);
    }).catch(reason => { if (!disposed) error = String(reason); })
      .finally(() => { clearTimeout(timer); if (!disposed) loading = false; });
    return () => { disposed = true; clearTimeout(timer); };
  });
</script>

<div class="dialog-overlay">
  <div class="diff-dialog" role="dialog" aria-modal="true" aria-labelledby="diff-title" tabindex="-1" bind:this={dialog}
    onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); } }}>
    <header><h2 id="diff-title">File diff</h2><button class="dialog-btn" onclick={onClose}>Close</button></header>
    <div class="paths"><div><strong>− Left:</strong> {pair.left.path}</div><div><strong>+ Right:</strong> {pair.right.path}</div></div>
    <p class="notice">Read-only, live file contents—not a snapshot of the earlier scan. UTF-8 text up to 512 KiB per side. S3 reads may incur download charges.</p>
    {#if loading}<p role="status">Reading both files…</p>
    {:else if error}<ErrorNotice {error} context="Could not compare files" />
    {:else if binary}<p>Binary or non-UTF-8 file detected. Text diff is unavailable; download the files to compare externally.</p>
    {:else if rows.every(row => row.kind === 'same')}<p>Contents now match. Earlier metadata or contents may have differed.</p>
    {:else}
      <div class="diff-content" aria-label="Unified file diff">
        {#each rows as row, index (index)}
          <div class="diff-line" class:added={row.kind === 'added'} class:removed={row.kind === 'removed'}>
            <span class="line-number">{row.left ?? ''}</span><span class="line-number">{row.right ?? ''}</span>
            <span class="marker">{row.kind === 'added' ? '+' : row.kind === 'removed' ? '−' : ' '}</span><span class="line-text">{row.text}</span>
          </div>
          {#if row.noNewline}<div class="newline-note">\ No newline at end of file</div>{/if}
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .diff-dialog { width: min(1100px, 94vw); height: 85vh; display: flex; flex-direction: column; padding: 18px; background: var(--bg-primary); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: 8px; }
  header { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0 0 12px; font-size: 18px; }
  .paths { font-size: 12px; overflow-wrap: anywhere; }
  .notice { color: var(--text-secondary); font-size: 12px; }
  .diff-content { overflow: auto; flex: 1; font: 12px/1.6 monospace; user-select: text; }
  .diff-line { display: flex; min-width: max-content; white-space: pre; }
  .added { background: color-mix(in srgb, var(--git-added) 18%, transparent); }
  .removed { background: color-mix(in srgb, var(--git-deleted) 18%, transparent); }
  .line-number { width: 6ch; text-align: right; padding-right: 1ch; flex-shrink: 0; color: var(--text-secondary); user-select: none; }
  .marker { width: 2ch; flex-shrink: 0; user-select: none; }
  .newline-note { color: var(--text-secondary); padding-left: 16ch; }
</style>
