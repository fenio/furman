<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { PanelBackend } from '#lib/types/index.js';
  import { canComparePanes, defaultComparisonMode, parseComparisonExcludes, type ComparisonMode } from '#lib/utils/comparison-options.js';

  interface Props {
    leftPath: string;
    leftBackend: PanelBackend;
    rightPath: string;
    rightBackend: PanelBackend;
    onCompare: (mode: ComparisonMode, excludes: string[]) => void;
    onClose: () => void;
  }

  let { leftPath, leftBackend, rightPath, rightBackend, onCompare, onClose }: Props = $props();
  let mode = $state<ComparisonMode>(untrack(() => defaultComparisonMode(leftBackend, rightBackend)));
  let excludeText = $state('');
  const supported = $derived(canComparePanes(leftBackend, rightBackend));
  let dialog: HTMLDivElement;
  onMount(() => dialog.focus());

  function compare() {
    if (supported && leftPath && rightPath) onCompare(mode, parseComparisonExcludes(excludeText));
  }
</script>

<div class="dialog-overlay">
  <div class="dialog-box" role="dialog" aria-modal="true" aria-labelledby="compare-title" tabindex="-1" bind:this={dialog}
    onkeydown={(event) => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
    }}>
    <h2 id="compare-title">Compare panes</h2>
    <form onsubmit={(event) => { event.preventDefault(); compare(); }}>
      <dl>
        <dt>Left pane</dt><dd>{leftPath} ({leftBackend})</dd>
        <dt>Right pane</dt><dd>{rightPath} ({rightBackend})</dd>
      </dl>
      {#if !supported}
        <p role="alert">Comparison currently supports local folders and S3 buckets/prefixes, not SFTP or archives.</p>
      {/if}
      <fieldset>
        <legend>Comparison method</legend>
        <label><input type="radio" bind:group={mode} value="sha256" /> Contents — SHA-256</label>
        <p>Compare actual file contents, ignoring modification dates. Recommended for copied buckets. Reads every included file on both sides.</p>
        <label><input type="radio" bind:group={mode} value="size_mtime" /> Metadata — size and modification date</label>
        <p>Fast, but does not verify contents. S3 copies usually have new dates, so identical copies may be marked modified.</p>
      </fieldset>
      <label for="compare-excludes">Exclude patterns (optional, comma-separated)</label>
      <input id="compare-excludes" type="text" bind:value={excludeText} placeholder=".DS_Store, *.tmp, node_modules/**" />
      <p>Compares the current pane roots recursively. No exclusions unless specified; current file contents only, not version history. Neither side is modified.</p>
      {#if mode === 'sha256' && (leftBackend === 's3' || rightBackend === 's3')}
        <p>S3 contents are downloaded for hashing; download/egress charges may apply. Keep both sides stable during verification.</p>
      {/if}
      <div class="dialog-footer">
        <button type="button" onclick={onClose}>Cancel</button>
        <button type="submit" class="primary" disabled={!supported || !leftPath || !rightPath}>Compare</button>
      </div>
    </form>
  </div>
</div>

<style>
  .dialog-overlay { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; background: rgba(0, 0, 0, 0.5); z-index: 100; }
  .dialog-box { width: 560px; max-width: 90vw; max-height: 90vh; overflow: auto; padding: 20px; background: var(--dialog-bg); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: var(--radius-lg); box-shadow: var(--shadow-dialog); }
  h2 { margin: 0 0 16px; font-size: 16px; }
  dl { margin: 0 0 16px; }
  dt { font-weight: 600; }
  dd { margin: 4px 0 12px; overflow-wrap: anywhere; color: var(--text-secondary); }
  fieldset { border: 1px solid var(--border-subtle); padding: 12px; margin: 0 0 16px; }
  legend { padding: 0 6px; }
  label { display: block; font-size: 13px; }
  p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  input[type='text'] { width: 100%; box-sizing: border-box; margin-top: 8px; padding: 6px; color: var(--text-primary); background: var(--bg-surface); border: 1px solid var(--border-subtle); }
  .dialog-footer { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  button { padding: 6px 16px; color: var(--text-primary); background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: 4px; cursor: pointer; }
  button.primary { background: var(--text-accent); color: var(--bg-primary); }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
