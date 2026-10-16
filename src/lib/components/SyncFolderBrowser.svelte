<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import type { FileEntry } from '#lib/types/index.js';
  import { listDirectory } from '#lib/services/tauri.js';
  import { s3ListObjectsPage } from '#lib/services/s3.js';
  import { locationError, s3Folder, type SyncLocation } from '#lib/utils/sync-workflow.js';
  import ErrorNotice from './ErrorNotice.svelte';

  let { location, onChoose, onCancel }: { location: SyncLocation; onChoose: (path: string) => void; onCancel: () => void } = $props();
  const scope = untrack(() => ({ ...location }));
  let draft = $state(untrack(() => location.path || (location.backend === 'local' ? '/' : `s3://${location.bucket}/`)));
  let current = $state('');
  let folders = $state.raw<FileEntry[]>([]);
  let busy = $state(false);
  let failure = $state<unknown>(null);
  let token = $state<string | null>(null);
  let pages = $state(0);
  let capped = $state(false);
  let browser = $state<HTMLDivElement>();
  let active = true;
  let generation = 0;
  let tokens = new SvelteSet<string>();
  const canChoose = $derived(!!current && draft === current && !busy && !failure);
  const canLoadMore = $derived(!!token && !busy && !capped && pages < 20);
  onMount(() => { browser?.focus(); void navigate(draft); return () => { active = false; generation++; }; });

  async function navigate(path: string) {
    if (!active || busy) return;
    const invalid = locationError({ ...scope, path });
    if (invalid) { failure = new Error(invalid); return; }
    generation++; current = ''; draft = path; folders = []; pages = 0; token = null; capped = false; tokens = new SvelteSet(); failure = null;
    await read(path, null);
  }
  async function read(path: string, continuation: string | null) {
    if (!active || busy) return;
    const request = generation; busy = true; failure = null;
    try {
      const page = scope.backend === 'local'
        ? { ...await listDirectory(path, true), next_token: null }
        : await s3ListObjectsPage(scope.s3Id, s3Folder({ ...scope, path })!.prefix, continuation);
      if (!active || request !== generation) return;
      if (page.next_token !== null && (!page.next_token || tokens.has(page.next_token))) throw new Error('Folder listing returned a repeated/empty continuation token. Refresh or choose another path; previous folders were kept.');
      const seen = new SvelteSet(folders.map(folder => folder.path));
      const fresh = page.entries.filter(entry => entry.is_dir && !entry.is_symlink && entry.name !== '..' && !seen.has(entry.path) && (seen.add(entry.path), true));
      capped = folders.length + fresh.length > 1000;
      folders = [...folders, ...fresh].slice(0, 1000).sort((a, b) => a.name.localeCompare(b.name));
      pages++; current = path; token = page.next_token;
      if (token) tokens.add(token);
    } catch (error) { if (active && request === generation) failure = error; }
    finally { if (active && request === generation) busy = false; }
  }
  async function more() { if (canLoadMore && current && draft === current) await read(current, token); }
  async function up() {
    if (!active || busy) return;
    if (scope.backend === 'local') { const path = (current || draft).replace(/\/+$/, ''); await navigate(path.slice(0, path.lastIndexOf('/')) || '/'); }
    else {
      const folder = s3Folder({ ...scope, path: current || draft });
      if (!folder) return;
      const prefix = folder.prefix.slice(0, -1); const end = prefix.lastIndexOf('/');
      await navigate(`s3://${folder.bucket}/${end < 0 ? '' : prefix.slice(0, end + 1)}`);
    }
  }
  function choose() { if (active && canChoose) onChoose(current); }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Escape cancels this focusable browser and restores focus to its opener.) -->
<div class="browser" role="region" aria-label="Choose a folder" tabindex="-1" bind:this={browser} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onCancel(); } }}>
  <h4>Browse {scope.backend === 'local' ? 'local folders' : `folders in ${scope.bucket}`}</h4>
  <p>Open a folder, then click <strong>Use this folder</strong>. Browsing reads directory metadata only; it never compares contents, saves a job or transfers files.{scope.backend === 's3' ? ' S3 LIST request charges may apply; pages load only on request.' : ' Symbolic-link folders are not followed.'}</p>
  <label>Folder path <input type="text" bind:value={draft} disabled={busy} onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); void navigate(draft); } }} /></label>
  <div class="actions"><button type="button" onclick={up} disabled={busy}>↑ Parent folder</button><button type="button" onclick={() => navigate(draft)} disabled={busy}>Open folder / refresh</button></div>
  {#if busy}<p role="status">Reading folders… You can cancel; an in-flight metadata request may finish, but its result will be discarded.</p>{/if}
  <div class="folders">{#each folders as folder (folder.path)}<button type="button" disabled={busy} onclick={() => navigate(folder.path)}><span aria-hidden="true">📁</span> {folder.name} <span aria-hidden="true">→</span></button>{/each}</div>
  {#if current && !folders.length && !busy && !failure}<p>No subfolders in the loaded results. You can use this folder or load another S3 page if available.</p>{/if}
  {#if token}<p>Partial folder listing · {pages} page(s) loaded. Choosing a location is not a complete comparison.</p><button type="button" onclick={more} disabled={!canLoadMore || draft !== current}>{failure ? 'Retry folder page' : 'Load next folder page'}</button>{/if}
  {#if capped || (token && pages >= 20)}<p role="status">Folder browser limit reached (1,000 visible folders / 20 S3 pages). Enter a narrower folder path; nothing was scanned recursively.</p>{/if}
  {#if failure}<ErrorNotice error={failure} context="Folder browsing failed — no files were changed" />{/if}
  <div class="actions"><button type="button" onclick={onCancel}>Cancel browsing</button><button type="button" class="primary" onclick={choose} disabled={!canChoose}>Use this folder</button></div>
</div>

<style>
  .browser { margin-top: 15px; padding: 15px; border: 1px solid var(--border-subtle); border-radius: 6px; background: var(--bg-surface); }
  h4 { margin: 0 0 8px; font-size: 14px; } p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  label { display: block; font-size: 12px; } input { display: block; width: 100%; box-sizing: border-box; padding: 8px; margin-top: 6px; color: var(--text-primary); background: var(--dialog-bg); border: 1px solid var(--border-subtle); }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; justify-content: space-between; margin-top: 12px; }
  button { padding: 7px 10px; border: 1px solid var(--border-subtle); border-radius: 4px; color: var(--text-primary); background: var(--dialog-bg); font: inherit; font-size: 12px; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; } button.primary { background: var(--text-accent); color: var(--bg-primary); }
  .folders { max-height: 220px; overflow: auto; margin-top: 12px; display: flex; flex-direction: column; gap: 3px; } .folders button { text-align: left; display: flex; gap: 8px; }
</style>
