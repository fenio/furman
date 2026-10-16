<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { SyncEntry } from '#lib/types/index.js';
  import { defaultComparisonMode } from '#lib/utils/comparison-options.js';
  import { locationError, syncLocationError, s3Folder, type SyncLocation, type SyncOptions } from '#lib/utils/sync-workflow.js';
  import { listSavedJobs, saveJobDefinition, type SavedJobsReport, type SavedJobDefinition } from '#lib/services/saved-jobs.js';
  import { appState } from '#lib/state/app.svelte.js';
  import SyncLocationPicker from './SyncLocationPicker.svelte';
  import SyncDialog from './SyncDialog.svelte';
  import JobComparisonSession from './JobComparisonSession.svelte';
  import SavedSyncJobs from './SavedSyncJobs.svelte';
  import JobHistory from './JobHistory.svelte';
  import ErrorNotice from './ErrorNotice.svelte';

  let { source: initialSource, destination: initialDestination, locations = [], options, onSync, onClose }: {
    source: SyncLocation; destination: SyncLocation; locations?: SyncLocation[]; options?: SyncOptions | null;
    onSync: (entries: SyncEntry[], source: SyncLocation, destination: SyncLocation) => void; onClose: () => void;
  } = $props();
  // Capture context once: pane navigation and comparison completion cannot
  // silently retarget an open guide or resurrect its old approvals.
  const choices = untrack(() => locations.filter((location, index) => location.backend !== 's3' || locations.findIndex(other => other.backend === 's3' && other.s3Id === location.s3Id) === index).map(location => ({ ...location })));
  function capture(location: SyncLocation): SyncLocation {
    if (location.backend === 'local') return { backend: 'local', path: location.path, s3Id: '' };
    const connected = choices.find(choice => location.backend === 's3' && choice.s3Id === location.s3Id);
    return { ...connected, ...location, profileId: location.profileId ?? connected?.profileId, bucket: location.bucket ?? connected?.bucket ?? s3Folder(location)?.bucket };
  }
  let source = $state<SyncLocation>(untrack(() => capture(initialSource)));
  let destination = $state<SyncLocation>(untrack(() => capture(initialDestination)));
  const settings = untrack(() => options ? { ...options, exclusions: [...options.exclusions] } : {
    mode: defaultComparisonMode(initialSource.backend, initialDestination.backend),
    exclusions: appState.syncExcludePatterns.split(',').map(value => value.trim()).filter(Boolean),
  });
  let step = $state<1 | 2 | 3>(1);
  let report = $state.raw<SavedJobsReport | null>(null);
  let saved = $state.raw<SavedJobDefinition | null>(null);
  let saveSetup = $state(false);
  let name = $state('');
  let exclusions = $state(settings.exclusions.join(', '));
  let reviewMode = $state(settings.mode);
  let busy = $state(false);
  let failure = $state<unknown>(null);
  let active = true;
  let manage = $state(false);
  let dialog = $state<HTMLDivElement>();
  const rootsError = $derived(syncLocationError(source, destination));
  const sourceProfile = $derived(report?.profiles.find(profile => profile.id === source.profileId));
  const destinationProfile = $derived(report?.profiles.find(profile => profile.id === destination.profileId));
  const canSave = $derived(!!report && destinationProfile?.configuration && s3Folder(destination)?.bucket === destinationProfile.configuration.bucket
    && (source.backend === 'local' || (sourceProfile?.configuration && s3Folder(source)?.bucket === sourceProfile.configuration.bucket)));

  onMount(() => { dialog?.focus(); void load(); return () => { active = false; }; });
  async function load() {
    if (!active || busy) return; busy = true; failure = null;
    try { const result = await listSavedJobs(); if (active) report = result; }
    catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }
  function chooseSaved(id: string) {
    if (!active || busy) return;
    const entry = report?.jobs.find(job => job.definition.id === id);
    if (!entry) return;
    if (entry.status !== 'definition_valid') { failure = new Error(`${entry.reason} Open “Manage saved transfers” to explicitly review and rebind it.`); return; }
    saved = structuredClone(entry.definition); failure = null; step = 3;
  }
  function changeSource(value: SyncLocation) { if (!active || busy) return; if (value.backend !== source.backend) reviewMode = defaultComparisonMode(value.backend, destination.backend); source = capture(value); saveSetup = false; failure = null; }
  function changeDestination(value: SyncLocation) { if (!active || busy) return; if (value.backend !== destination.backend) reviewMode = defaultComparisonMode(source.backend, value.backend); destination = capture(value); saveSetup = false; failure = null; }
  function nextSource() { if (active && !locationError(source) && !busy) step = 2; }
  async function nextDestination() {
    if (!active || busy || rootsError) return;
    failure = null;
    if (!saveSetup) { step = 3; return; }
    if (!canSave || !report || !name.trim()) return;
    busy = true;
    try {
      const result = await saveJobDefinition({ expected_revision: report.revision, expected_configuration_revision: report.configuration_revision, id: null, name: name.trim(),
        source: source.backend === 'local' ? { kind: 'local', path: source.path } : { kind: 's3', profile_id: source.profileId!, prefix: s3Folder(source)!.prefix },
        destination_profile_id: destination.profileId!, destination_prefix: s3Folder(destination)!.prefix,
        exclude_patterns: exclusions.split(',').map(value => value.trim()).filter(Boolean), acknowledged: true });
      if (active) {
        report = result;
        const created = result.jobs.find(job => job.definition.name === name.trim());
        if (!created || created.status !== 'definition_valid') throw new Error(created?.reason ?? 'Saved transfer was not returned; refresh and select it explicitly.');
        saved = structuredClone(created.definition); step = 3;
      }
    } catch (error) { if (active) failure = error; }
    finally { if (active) { busy = false; saveSetup = false; } }
  }
  function back() { step = saved ? 1 : 2; saved = null; saveSetup = false; failure = null; }
  function transfer(entries: SyncEntry[]) {
    if (!active || step !== 3 || saved || rootsError) return;
    onSync(entries, { ...source }, { ...destination });
  }
  function savedSource(job: SavedJobDefinition) { return job.source.kind === 'local' ? job.source.path : `s3://${job.source.pin.configuration.bucket}/${job.source.pin.prefix}`; }
</script>

<div class="overlay" role="presentation">
  <div class="wizard" role="dialog" aria-modal="true" aria-labelledby="sync-guide-title" tabindex="-1" bind:this={dialog} onkeydown={(event) => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
  }}>
    <header><h2 id="sync-guide-title">Sync files</h2><button type="button" onclick={onClose} aria-label="Close Sync guide">×</button></header>
    <ol aria-label="Sync setup steps"><li aria-current={step === 1 ? 'step' : undefined}>1 · Source</li><li aria-current={step === 2 ? 'step' : undefined}>2 · Destination</li><li aria-current={step === 3 ? 'step' : undefined}>3 · Review &amp; transfer</li></ol>
    <div class="body">
      {#if settings.fromComparison && !saved}<p class="notice">Locations, comparison method and exclusions came from Compare. Its results are read-only: click Compare again here before reviewing a transfer. No previous selection or approval is reused.</p>{/if}
      {#if step === 1}
        <h3>Where are the original files?</h3>
        <p>Start with the active pane’s folder, or choose another location. This is a one-way copy/update—not a mirror; destination-only files stay untouched.</p>
        <SyncLocationPicker label="Source" value={source} locations={choices} disabled={busy} onChange={changeSource} />
        {#if locationError(source)}<p role="status">{locationError(source)}</p>{/if}
        <div class="actions"><button type="button" onclick={onClose}>Cancel</button><button type="button" class="primary" onclick={nextSource} disabled={busy || !!locationError(source)}>Next: destination →</button></div>
        <hr />
        <label>Or use a saved transfer <select value="" disabled={busy || !report} onchange={(event) => chooseSaved(event.currentTarget.value)}><option value="">Choose a saved transfer…</option>{#each report?.jobs ?? [] as entry (entry.definition.id)}<option value={entry.definition.id}>{entry.definition.name}{entry.status === 'needs_rebind' ? ' — needs review' : ''}</option>{/each}</select></label>
        <details><summary>Manage saved transfers and local history</summary><button type="button" onclick={() => { manage = !manage; }}>Open / close management</button>{#if manage}<SavedSyncJobs defaultLocalPath={source.backend === 'local' ? source.path : ''} />{:else}<JobHistory labels={Object.fromEntries((report?.jobs ?? []).map(entry => [entry.definition.id, entry.definition.name]))} />{/if}</details>
        <button type="button" onclick={load} disabled={busy}>Refresh saved transfers</button>
      {:else if step === 2}
        <h3>Where should the files go?</h3>
        <p class="direction"><strong>From:</strong> {source.path}</p>
        <SyncLocationPicker label="Destination" value={destination} locations={choices} disabled={busy} onChange={changeDestination} />
        {#if rootsError}<p class="notice" role="status">{rootsError}</p>{/if}
        <details><summary>Save this setup for next time (optional)</summary>
          <p>One-off transfers use the current connections. Reusable jobs use saved access-key S3 profiles, mandatory SHA-256, separate approvals and durable history (up to 1 GiB/file). Saved profile settings—not unsaved live credentials—are authoritative; review their origins again before connecting. Write policy and approvals are chosen afresh for each run, not saved. No scheduling.</p>
          {#if canSave}
            <label>Name <input disabled={busy} type="text" maxlength="80" bind:value={name} oninput={() => { saveSetup = false; }} /></label>
            <label>Explicit exclusions <input disabled={busy} type="text" bind:value={exclusions} oninput={() => { saveSetup = false; }} /></label>
            <p>Saved source: {sourceProfile?.name ?? source.path} ({sourceProfile?.configuration?.endpoint ?? sourceProfile?.configuration?.region ?? 'local folder'}); destination: {destinationProfile?.name}, {destinationProfile?.configuration?.endpoint ?? destinationProfile?.configuration?.region}. Destination SSE-C: {destinationProfile?.configuration?.sse_customer_key_enabled ? 'enabled with its own saved key' : 'disabled'}. Saving inspects local directory metadata and stores configuration pins, not credentials or approval.</p>
            <label><input disabled={busy} type="checkbox" bind:checked={saveSetup} /> I reviewed this source → destination and exclusions. Save as a reusable job and continue; do not connect, scan or transfer yet.</label>
          {:else}<p>Saving requires a saved, eligible access-key S3 destination profile (and a saved profile for an S3 source). Connect those profiles in the panes first. Local destinations and unsaved/dynamic-auth connections remain available as one-off transfers.</p>{/if}
        </details>
        <div class="actions"><button type="button" onclick={() => { step = 1; saveSetup = false; }} disabled={busy}>← Source</button><button type="button" class="primary" onclick={nextDestination} disabled={busy || !!rootsError || (saveSetup && (!canSave || !name.trim()))}>{busy ? 'Saving…' : saveSetup ? 'Save job and continue →' : 'Next: review →'}</button></div>
      {:else if saved}
        <h3>{saved.name}</h3><p class="direction"><strong>From:</strong> {savedSource(saved)}<br /><strong>To:</strong> s3://{saved.destination.configuration.bucket}/{saved.destination.prefix}</p>
        <p>Source service: {saved.source.kind === 's3' ? saved.source.pin.configuration.endpoint ?? `AWS region ${saved.source.pin.configuration.region}` : 'local filesystem'}. Destination service: {saved.destination.configuration.endpoint ?? `AWS region ${saved.destination.configuration.region}`}; SSE-C {saved.destination.configuration.sse_customer_key_enabled ? 'enabled with its own saved key' : 'disabled'}.</p>
        <p>Saved transfer · SHA-256 · exclusions: {saved.exclude_patterns.join(', ') || '(none)'}. Nothing connects automatically.</p>
        <button type="button" onclick={back}>← Choose another transfer (cancel current session)</button>
        {#key saved.id + ':' + saved.revision}<JobComparisonSession definition={saved} guided />{/key}
      {:else}
        <h3>Review what will change</h3><p class="direction"><strong>From:</strong> {source.path}<br /><strong>To:</strong> {destination.path}</p>
        <SyncDialog embedded sourceBackend={source.backend} sourcePath={source.path} sourceS3Id={source.s3Id} destBackend={destination.backend} destPath={destination.path} destS3Id={destination.s3Id} initialMode={reviewMode} initialExclusions={exclusions.split(',').map(value => value.trim()).filter(Boolean)} onSync={transfer} onClose={back} />
      {/if}
      {#if failure}<ErrorNotice error={failure} context="Sync setup / saved transfers" />{/if}
    </div>
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; background: rgba(0,0,0,.5); backdrop-filter: blur(8px); z-index: 100; }
  .wizard { width: 780px; max-width: 94vw; max-height: 90vh; display: flex; flex-direction: column; border: 1px solid var(--dialog-border); border-radius: var(--radius-lg); background: var(--dialog-bg); color: var(--text-primary); box-shadow: var(--shadow-dialog); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 14px 22px; border-bottom: 1px solid var(--border-subtle); }
  h2 { font-size: 17px; margin: 0; } h3 { margin: 0 0 12px; font-size: 17px; }
  ol { display: flex; list-style: none; gap: 18px; margin: 0; padding: 16px 22px; font-size: 13px; border-bottom: 1px solid var(--border-subtle); color: var(--text-secondary); }
  li[aria-current='step'] { color: var(--text-accent); font-weight: 600; }
  .body { padding: 22px; overflow-y: auto; } p { color: var(--text-secondary); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
  .direction, .notice { padding: 12px; background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: 5px; }
  label { display: block; margin: 12px 0; font-size: 13px; } select, input[type='text'] { display: block; width: 100%; box-sizing: border-box; margin-top: 7px; padding: 8px; background: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); }
  button { font: inherit; font-size: 13px; padding: 8px 12px; background: var(--bg-surface); color: var(--text-primary); border: 1px solid var(--border-subtle); border-radius: 5px; cursor: pointer; }
  button.primary { background: var(--text-accent); color: var(--bg-primary); } button:disabled { opacity: .5; cursor: default; }
  .actions { display: flex; justify-content: space-between; gap: 12px; margin: 22px 0 8px; } details { margin: 18px 0; } summary { cursor: pointer; font-size: 13px; } hr { border: 0; border-top: 1px solid var(--border-subtle); margin: 25px 0; }
</style>
