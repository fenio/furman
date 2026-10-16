<script lang="ts">
  import type { SyncLocation } from '#lib/utils/sync-workflow.js';
  import SyncFolderBrowser from './SyncFolderBrowser.svelte';
  let { label, value, locations, onChange, disabled = false }: { label: string; value: SyncLocation; locations: SyncLocation[]; onChange: (value: SyncLocation) => void; disabled?: boolean } = $props();
  const choice = $derived(locations.findIndex(location => location.backend === value.backend && location.s3Id === value.s3Id));
  const connections = $derived(locations.filter(location => location.backend === 's3'));
  const localFolders = $derived(locations.filter(location => location.backend === 'local'));
  let browsing = $state(false);
  let browseButton = $state<HTMLButtonElement>();
  function change(next: SyncLocation) { if (disabled) return; browsing = false; onChange({ ...next }); }
  function closeBrowser() { browsing = false; browseButton?.focus(); }
</script>

<div class="types" aria-label={`${label} storage type`}>
  <button type="button" {disabled} aria-pressed={value.backend === 'local'} onclick={() => { if (value.backend !== 'local') change(localFolders[0] ?? { backend: 'local', path: '/', s3Id: '' }); }}>Local folder</button>
  {#if connections.length}<button type="button" {disabled} aria-pressed={value.backend === 's3'} onclick={() => { if (value.backend !== 's3' || !connections.some(connection => connection.s3Id === value.s3Id)) change(connections[0]); }}>Connected S3 bucket</button>{/if}
</div>
{#if value.backend === 's3'}
  {#if connections.length > 1}<label>S3 connection <select {disabled} value={String(choice)} onchange={(event) => change(locations[Number(event.currentTarget.value)])}>{#each locations as location, index (index)}{#if location.backend === 's3'}<option value={String(index)}>{location.bucket ?? location.path} — {location.label ?? `connection ${index + 1}`}</option>{/if}{/each}</select></label>
  {:else}<p><strong>S3 connection:</strong> {value.bucket ?? value.path} — {value.label ?? locations[choice]?.label ?? 'original connection'}</p>{/if}
{/if}
<label>{value.backend === 'local' ? 'Absolute folder path' : 'S3 bucket/folder path'}
  <input {disabled} type="text" value={value.path} placeholder={value.backend === 'local' ? '/Users/you/downloads' : 's3://bucket/folder/'} oninput={(event) => change({ ...value, path: event.currentTarget.value })} />
</label>
<button type="button" {disabled} bind:this={browseButton} onclick={() => { browsing = !browsing; }}>{browsing ? 'Close folder browser' : 'Browse folders…'}</button>
{#if !browsing && value.backend === 'local' && localFolders.length}<div class="shortcuts"><span>Pane shortcuts (starting points, not your only choices):</span>{#each localFolders as folder, index (index)}<button type="button" {disabled} onclick={() => change(folder)}>{folder.label ?? `Pane ${index + 1}`}: {folder.path}</button>{/each}</div>{/if}
{#if !connections.length}<p>No connected S3 bucket is available in the panes. To select S3, connect a bucket in a pane and reopen this guide, or choose an existing saved transfer in step 1. Local folders can be browsed or typed here.</p>{:else if value.backend === 's3'}<p>Browse or type a folder within this bucket; prefixes must end in /. Other accounts/buckets must first be connected in a pane. Choosing a location does not compare or transfer.</p>{/if}
{#if browsing && !disabled}{#key value.backend + ':' + value.s3Id + ':' + value.path}<SyncFolderBrowser location={{ ...value }} onChoose={(path) => { change({ ...value, path }); closeBrowser(); }} onCancel={closeBrowser} />{/key}{/if}

<style>
  label { display: block; margin: 16px 0; font-size: 13px; }
  input, select { display: block; width: 100%; box-sizing: border-box; margin-top: 7px; padding: 9px; border: 1px solid var(--border-subtle); border-radius: 5px; background: var(--bg-surface); color: var(--text-primary); font: inherit; }
  p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  button { font: inherit; font-size: 13px; padding: 8px 12px; border: 1px solid var(--border-subtle); border-radius: 4px; color: var(--text-primary); background: var(--bg-surface); cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; } button[aria-pressed='true'] { border-color: var(--text-accent); }
  .types { display: flex; gap: 8px; margin-top: 16px; } .shortcuts { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; margin-top: 14px; font-size: 12px; color: var(--text-secondary); }
</style>
