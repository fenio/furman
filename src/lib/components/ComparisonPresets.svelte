<script lang="ts">
  import { onMount } from 'svelte';
  import { loadComparisonPresets, saveComparisonPresets, type PresetReport } from '#lib/services/comparison-presets.js';
  import { parseComparisonExcludes, type ComparisonMode } from '#lib/utils/comparison-options.js';
  import ErrorNotice from './ErrorNotice.svelte';

  let { mode, excludeText, onApply }: {
    mode: ComparisonMode;
    excludeText: string;
    onApply: (mode: ComparisonMode, exclusions: string[]) => void;
  } = $props();

  let report = $state.raw<PresetReport | null>(null);
  let selectedId = $state('');
  let name = $state('');
  let busy = $state(false);
  let ready = $state(false);
  let failure = $state<unknown>(null);
  let status = $state('');
  let confirmDelete = $state(false);
  let active = true;
  const selected = $derived(report?.presets.find(preset => preset.id === selectedId));

  onMount(() => {
    void refresh();
    return () => { active = false; };
  });

  async function refresh() {
    if (busy) return;
    busy = true; ready = false; failure = null; status = ''; confirmDelete = false;
    try {
      const result = await loadComparisonPresets();
      if (active) { report = result; ready = true; selectedId = ''; name = ''; }
    } catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }

  function apply() {
    if (!ready || busy || !selected) return;
    onApply(selected.compare_mode, [...selected.exclude_patterns]);
    status = `Applied “${selected.name}” to the current dialog only. Review roots and options, then start a fresh comparison.`;
  }

  function handleKeydown(event: KeyboardEvent) {
    // This component can be inside Compare's form. Enter while naming a preset
    // must not implicitly submit that form and start a potentially costly scan.
    if (event.key === 'Enter' && (event.target as HTMLElement | null)?.matches('input[type="text"]')) {
      event.preventDefault();
      event.stopPropagation();
    }
  }

  async function mutate(action: 'create' | 'update' | 'delete') {
    if (!ready || busy || !report || (action !== 'create' && !selected) || (action === 'delete' && !confirmDelete)) return;
    const id = action === 'create' ? crypto.randomUUID() : selected!.id;
    const preset = { id, name: name.trim(), compare_mode: mode, exclude_patterns: parseComparisonExcludes(excludeText) };
    const next = action === 'create' ? [...report.presets, preset] : action === 'delete' ? report.presets.filter(entry => entry.id !== id) : report.presets.map(entry => entry.id === id ? preset : entry);
    busy = true; failure = null; status = '';
    try {
      const result = await saveComparisonPresets(report.revision, next);
      if (active) {
        report = result; selectedId = action === 'delete' ? '' : id; name = action === 'delete' ? '' : preset.name;
        confirmDelete = false;
        status = action === 'delete' ? 'Preset deleted. Current comparison options are unchanged.' : 'Preset saved. No comparison, connection or transfer was started.';
      }
    } catch (error) { if (active) failure = error; }
    finally { if (active) busy = false; }
  }
</script>

<details data-comparison-presets>
  <summary>Saved comparison / sync presets{busy ? ' — working…' : failure ? ' — error (expand for details)' : ''}</summary>
<fieldset>
  <legend>Preset options</legend>
  <p>Options only: method and explicit exclusions. No paths, connection identities, secrets, selections, deletions or overwrite approvals are saved. Presets use the roots shown in this dialog and the current scan budgets.</p>
  <div class="preset-actions">
    <label>Saved preset
      <select value={selectedId} disabled={!ready || busy} onchange={(event) => { selectedId = event.currentTarget.value; name = report?.presets.find(preset => preset.id === selectedId)?.name ?? ''; confirmDelete = false; status = ''; }}>
        <option value="">New preset / choose explicitly</option>
        {#each report?.presets ?? [] as preset (preset.id)}<option value={preset.id}>{preset.name}</option>{/each}
      </select>
    </label>
    <button type="button" onclick={refresh} disabled={busy}>{busy ? 'Working…' : 'Refresh presets'}</button>
    <button type="button" onclick={apply} disabled={!ready || busy || !selected}>Apply selected options</button>
  </div>
  {#if selected}
    <p>Saved method: <strong>{selected.compare_mode === 'sha256' ? 'SHA-256 (reads contents)' : 'Size + Date (contents not verified)'}</strong>. Saved exclusions: {selected.exclude_patterns.length ? selected.exclude_patterns.join(', ') : 'None'}.</p>
  {/if}
  <label>Preset name <input type="text" bind:value={name} onkeydown={handleKeydown} maxlength="80" disabled={!ready || busy} placeholder="e.g. Content check, no exclusions" /></label>
  <div class="preset-actions">
    <button type="button" onclick={() => mutate('create')} disabled={!ready || busy || !name.trim() || (report?.presets.length ?? 0) >= 100}>Save current options as new</button>
    <button type="button" onclick={() => mutate('update')} disabled={!ready || busy || !selected || !name.trim()}>Update selected from current options</button>
  </div>
  {#if selected}
    <label><input type="checkbox" bind:checked={confirmDelete} disabled={busy} /> Confirm deleting preset “{selected.name}”</label>
    <button type="button" onclick={() => mutate('delete')} disabled={!ready || busy || !confirmDelete}>Delete selected preset</button>
  {/if}
  {#if failure}<ErrorNotice error={failure} context="Comparison presets" />{/if}
  {#if status}<p role="status">{status}</p>{/if}
</fieldset>
</details>

<style>
  fieldset { margin: 0 0 12px; padding: 10px; border: 1px solid var(--border-subtle); min-width: 0; }
  details { flex-shrink: 0; } summary { cursor: pointer; font-size: 12px; margin-bottom: 8px; }
  legend, label { font-size: 12px; } label { display: block; margin: 6px 0; }
  p { color: var(--text-secondary); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
  .preset-actions { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  input[type='text'], select { max-width: 100%; box-sizing: border-box; color: var(--text-primary); background: var(--bg-surface); border: 1px solid var(--border-subtle); padding: 4px; }
  button { font: inherit; font-size: 12px; padding: 4px 8px; color: var(--text-primary); background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: var(--radius-sm); }
  button:disabled { opacity: 0.5; }
</style>
