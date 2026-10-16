<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { listSavedJobs, saveJobDefinition, deleteJobDefinition, type SavedJobsReport, type SavedJobDefinition } from '#lib/services/saved-jobs.js';
  import { parseComparisonExcludes } from '#lib/utils/comparison-options.js';
  import ErrorNotice from './ErrorNotice.svelte';
  import JobComparisonSession from './JobComparisonSession.svelte';
  import JobHistory from './JobHistory.svelte';

  let { defaultLocalPath = '', onOpenChange = () => {} }: { defaultLocalPath?: string; onOpenChange?: (open: boolean) => void } = $props();
  let detailsOpen = $state(false);
  let report = $state.raw<SavedJobsReport | null>(null);
  let selectedId = $state('');
  let name = $state('');
  let sourceKind = $state<'local' | 's3'>(untrack(() => defaultLocalPath ? 'local' : 's3'));
  let localPath = $state(untrack(() => defaultLocalPath));
  let sourceProfile = $state('');
  let sourcePrefix = $state('');
  let destinationProfile = $state('');
  let destinationPrefix = $state('');
  let excludeText = $state('');
  let acknowledged = $state(false);
  let confirmDelete = $state(false);
  let busy = $state(false);
  let ready = $state(false);
  let failure = $state<unknown>(null);
  let status = $state('');
  let active = true;
  const selected = $derived(report?.jobs.find(job => job.definition.id === selectedId));
  const sourceConfig = $derived(report?.profiles.find(profile => profile.id === sourceProfile)?.configuration);
  const destinationConfig = $derived(report?.profiles.find(profile => profile.id === destinationProfile)?.configuration);

  onMount(() => {
    void refresh();
    return () => { active = false; onOpenChange(false); };
  });
  function changed() { acknowledged = false; confirmDelete = false; status = ''; }
  function choose(id: string) {
    selectedId = id; changed();
    const job = report?.jobs.find(job => job.definition.id === id)?.definition;
    if (job) edit(job);
    else { name = ''; sourceProfile = ''; sourcePrefix = ''; destinationProfile = ''; destinationPrefix = ''; excludeText = ''; }
  }
  function edit(job: SavedJobDefinition) {
    name = job.name; sourceKind = job.source.kind;
    if (job.source.kind === 'local') { localPath = job.source.path; sourceProfile = ''; sourcePrefix = ''; }
    else { sourceProfile = job.source.pin.profile_id; sourcePrefix = job.source.pin.prefix; }
    destinationProfile = job.destination.profile_id; destinationPrefix = job.destination.prefix;
    excludeText = job.exclude_patterns.join(', ');
  }
  async function refresh() {
    if (busy) return;
    busy = true; ready = false; failure = null; changed();
    try { const result = await listSavedJobs(); if (active) { report = result; ready = true; choose(''); } }
    catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }
  async function save(asNew: boolean) {
    if (!ready || busy || !report || !acknowledged || !name.trim() || (!asNew && !selected)) return;
    busy = true; failure = null; status = '';
    try {
      const result = await saveJobDefinition({ expected_revision: report.revision, expected_configuration_revision: report.configuration_revision, id: asNew ? null : selectedId, name: name.trim(),
        source: sourceKind === 'local' ? { kind: 'local', path: localPath } : { kind: 's3', profile_id: sourceProfile, prefix: sourcePrefix },
        destination_profile_id: destinationProfile, destination_prefix: destinationPrefix,
        exclude_patterns: parseComparisonExcludes(excludeText), acknowledged });
      if (active) { report = result; choose(result.jobs.find(job => job.definition.name === name.trim())?.definition.id ?? ''); status = 'Definition saved. No credentials were read and no connection, comparison or transfer was started. Execution is not implemented.'; }
    } catch (error) { if (active) failure = error; }
    finally { if (active) { busy = false; acknowledged = false; } }
  }
  async function remove() {
    if (!ready || busy || !report || !selected || !confirmDelete) return;
    busy = true; failure = null; status = '';
    try { const result = await deleteJobDefinition(report.revision, selectedId, confirmDelete); if (active) { report = result; choose(''); status = 'Definition deleted; current Sync roots and options are unchanged.'; } }
    catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }
</script>

<details data-saved-jobs bind:open={detailsOpen} ontoggle={(event) => onOpenChange(event.currentTarget.open)}>
  <summary>Saved job definitions — explicit manual workflow{busy ? ' — working…' : failure ? ' — error (expand for details)' : ''}</summary>
  <fieldset disabled={busy}>
    <legend>Pinned definitions (foundation)</legend>
    <p>These fields are separate from the current Sync operation. Saving/rebinding inspects saved configuration and local directory metadata only. It does not read keychain secrets, connect, apply roots to this dialog, scan or transfer. Select a valid saved revision for explicit binding/comparison/preparation below. Only eligible local → S3 / S3 → S3 creates can run after separate native execution review and approval; scheduling is disabled.</p>
    <label>Definition
      <select value={selectedId} disabled={!ready} onchange={(event) => choose(event.currentTarget.value)}>
        <option value="">New definition / choose explicitly</option>
        {#each report?.jobs ?? [] as job (job.definition.id)}<option value={job.definition.id}>{job.definition.name} — {job.status}</option>{/each}
      </select>
    </label>
    <button type="button" onclick={refresh}>Refresh definitions and profiles</button>
    {#if selected}
      <p role="status">{selected.status}: {selected.reason}</p>
      <p>Saved source: {selected.definition.source.kind === 'local' ? selected.definition.source.path : `s3://${selected.definition.source.pin.configuration.bucket}/${selected.definition.source.pin.prefix}`}</p>
      {#if selected.definition.source.kind === 's3'}<p>Saved source profile: {selected.definition.source.pin.profile_id}; {selected.definition.source.pin.configuration.endpoint ?? 'AWS region intent (effective routing not bound)'}; region {selected.definition.source.pin.configuration.region}; SSE-C {selected.definition.source.pin.configuration.sse_customer_key_enabled ? 'enabled' : 'disabled'}.</p>{/if}
      <p>Saved destination: s3://{selected.definition.destination.configuration.bucket}/{selected.definition.destination.prefix} — {selected.definition.destination.configuration.endpoint ?? `AWS configured region ${selected.definition.destination.configuration.region} (effective routing not bound)`}</p>
      <p>Saved destination profile: {selected.definition.destination.profile_id}; region {selected.definition.destination.configuration.region}; SSE-C {selected.definition.destination.configuration.sse_customer_key_enabled ? 'enabled' : 'disabled'}.</p>
    {/if}
    <label>Name <input type="text" bind:value={name} maxlength="80" oninput={changed} disabled={!ready} /></label>
    <label>Source type <select bind:value={sourceKind} onchange={changed} disabled={!ready}><option value="local">Local directory</option><option value="s3">Saved S3 profile</option></select></label>
    {#if sourceKind === 'local'}
      <label>Absolute local source directory <input type="text" bind:value={localPath} oninput={changed} disabled={!ready} /></label>
    {:else}
      <label>Source saved profile <select bind:value={sourceProfile} onchange={changed} disabled={!ready}>
        <option value="">Choose source explicitly</option>
        {#each report?.profiles ?? [] as profile (profile.id)}<option value={profile.id} disabled={!profile.configuration}>{profile.name} — {profile.configuration?.bucket ?? profile.unavailable}</option>{/each}
      </select></label>
      <label>Exact source prefix <input type="text" bind:value={sourcePrefix} oninput={changed} disabled={!ready} /></label>
      {#if sourceConfig}<p>Source → s3://{sourceConfig.bucket}/{sourcePrefix} — {sourceConfig.endpoint ?? `AWS configured region ${sourceConfig.region} (effective routing not bound)`}</p>{/if}
    {/if}
    <label>Destination saved S3 profile <select bind:value={destinationProfile} onchange={changed} disabled={!ready}>
      <option value="">Choose destination explicitly</option>
      {#each report?.profiles ?? [] as profile (profile.id)}<option value={profile.id} disabled={!profile.configuration}>{profile.name} — {profile.configuration?.bucket ?? profile.unavailable}</option>{/each}
    </select></label>
    <label>Exact destination prefix <input type="text" bind:value={destinationPrefix} oninput={changed} disabled={!ready} /></label>
    {#if destinationConfig}<p>Destination → s3://{destinationConfig.bucket}/{destinationPrefix} — {destinationConfig.endpoint ?? `AWS configured region ${destinationConfig.region} (effective routing not bound)`}; SSE-C {destinationConfig.sse_customer_key_enabled ? 'enabled' : 'disabled'}.</p>{/if}
    <label>Explicit exclusions (comma-separated, empty means none) <input type="text" bind:value={excludeText} oninput={changed} disabled={!ready} /></label>
    <p>Definitions fix SHA-256 and copy/update-only intent, never deletion or overwrite approval. Empty prefixes mean bucket root; nonempty folder prefixes must end with / and are never rewritten. Profile IDs are native-resolved configuration references, not proof of a principal or effective routing. Credentials remain in the OS keychain. Names, roots and non-secret endpoint configuration are saved in a private plaintext file.</p>
    <label><input type="checkbox" bind:checked={acknowledged} disabled={!ready} /> I reviewed this separate source → destination and acknowledge saving/rebinding configuration pins, not approving execution.</label>
    <div class="actions">
      <button type="button" onclick={() => save(true)} disabled={!ready || !acknowledged || !name.trim() || !destinationConfig || (sourceKind === 's3' && !sourceConfig)}>Save as new definition</button>
      <button type="button" onclick={() => save(false)} disabled={!ready || !selected || !acknowledged || !name.trim() || !destinationConfig || (sourceKind === 's3' && !sourceConfig)}>Save edits / explicitly rebind selected</button>
    </div>
    {#if selected}
      <label><input type="checkbox" bind:checked={confirmDelete} disabled={!ready} /> Confirm deleting “{selected.definition.name}” (definition only)</label>
      <button type="button" onclick={remove} disabled={!ready || !confirmDelete}>Delete definition</button>
    {/if}
  </fieldset>
  {#if selected && detailsOpen}
    {#if selected.status === 'definition_valid'}
      {#key selected.definition.id + ':' + selected.definition.revision}
        <JobComparisonSession definition={selected.definition} />
      {/key}
    {:else}<p>Explicitly rebind the saved definition above before requesting native connection validation or comparison.</p>{/if}
  {/if}
  {#if detailsOpen}<JobHistory labels={Object.fromEntries((report?.jobs ?? []).map(job => [job.definition.id, job.definition.name]))} />{/if}
  {#if failure}<ErrorNotice error={failure} context="Saved job definitions" />{/if}
  {#if status}<p role="status">{status}</p>{/if}
</details>

<style>
  details { flex-shrink: 0; } summary { cursor: pointer; font-size: 12px; margin-bottom: 8px; }
  fieldset { border: 1px solid var(--border-subtle); min-width: 0; padding: 10px; }
  label { display: block; margin: 8px 0; font-size: 12px; } p { font-size: 12px; color: var(--text-secondary); overflow-wrap: anywhere; line-height: 1.5; }
  input[type='text'], select { display: block; width: 100%; box-sizing: border-box; padding: 4px; background: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); }
  button { font: inherit; font-size: 12px; padding: 4px 8px; background: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; } button:disabled { opacity: 0.5; }
</style>
