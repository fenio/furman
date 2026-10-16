<script lang="ts">
  import type { SyncLocation } from '#lib/utils/sync-workflow.js';
  let { label, value, locations, onChange, disabled = false }: { label: string; value: SyncLocation; locations: SyncLocation[]; onChange: (value: SyncLocation) => void; disabled?: boolean } = $props();
  const choice = $derived(locations.findIndex(location => location.backend === value.backend && location.s3Id === value.s3Id));
</script>

<label>{label} location
  <select {disabled} value={value.backend === 'local' ? 'local' : String(choice)} onchange={(event) => {
    const choice = event.currentTarget.value;
    onChange(choice === 'local' ? { backend: 'local', path: '', s3Id: '' } : { ...locations[Number(choice)] });
  }}>
    <option value="local">Local folder</option>
    {#each locations as location, index (index)}
      {#if location.backend === 's3'}<option value={String(index)}>S3: {location.bucket ?? location.path} — {location.label ?? `connection ${index + 1}`}</option>{/if}
    {/each}
  </select>
</label>
<label>{value.backend === 'local' ? 'Absolute folder path' : 'S3 bucket/folder path'}
  <input {disabled} type="text" value={value.path} placeholder={value.backend === 'local' ? '/Users/you/downloads' : 's3://bucket/folder/'} oninput={(event) => onChange({ ...value, path: event.currentTarget.value })} />
</label>
{#if value.backend === 's3'}<p>Use a folder in this connection’s bucket; folder prefixes must end in /. To use another account or bucket, connect it in a pane first. Selecting a location does not connect or scan.</p>{/if}

<style>
  label { display: block; margin: 16px 0; font-size: 13px; }
  input, select { display: block; width: 100%; box-sizing: border-box; margin-top: 7px; padding: 9px; border: 1px solid var(--border-subtle); border-radius: 5px; background: var(--bg-surface); color: var(--text-primary); font: inherit; }
  p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
</style>
