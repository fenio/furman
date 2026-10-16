<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { SavedJobDefinition } from '#lib/services/saved-jobs.js';
  import type { ScanProgress } from '#lib/types/index.js';
  import { bindJobSession, scanJobSession, releaseJobSession, jobSessionStatus, jobManifestPage, prepareJobPreview, type JobRunStatus, type JobManifestRow, type JobPreviewPolicy } from '#lib/services/job-runs.js';
  import { error as logError } from '#lib/services/log.js';
  import ErrorNotice from './ErrorNotice.svelte';
  import ScanStatus from './ScanStatus.svelte';

  let { definition }: { definition: SavedJobDefinition } = $props();
  let approved = $state(false);
  let session = $state.raw<JobRunStatus | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let rows = $state.raw<JobManifestRow[]>([]);
  let busy = $state<'binding' | 'scanning' | 'preparing' | 'refreshing' | 'paging' | null>(null);
  let selectedIds = $state.raw<Set<string>>(new Set());
  let previewPolicy = $state<JobPreviewPolicy>('create_only');
  let snapshotMiB = $state(1024);
  let snapshotApproved = $state(false);
  function resetSelection() { selectedIds = new Set(); snapshotApproved = false; previewPolicy = 'create_only'; }
  function toggleEntry(id: string) {
    if (busy || session?.phase !== 'complete' || !rows.some(row => row.id === id && row.status !== 'destination_only')) return;
    selectedIds = selectedIds.has(id) ? new Set([...selectedIds].filter(entry => entry !== id)) : new Set([...selectedIds, id]);
    snapshotApproved = false;
  }
  let failure = $state<unknown>(null);
  let status = $state('');
  let active = true;
  let generation = 0;
  let opId = $state('');

  onDestroy(() => {
    active = false; generation++;
    const handle = session?.run_id ?? opId;
    if (handle) void releaseJobSession(handle).catch(() => logError('Could not release a read-only job session on close; its native expiry remains active.'));
  });
  async function bind() {
    if (!approved || busy || (session && session.phase !== 'stopped')) return;
    const current = ++generation;
    opId = crypto.randomUUID(); busy = 'binding'; failure = null; status = ''; rows = []; progress = null; session = null;
    resetSelection();
    try {
      const result = await bindJobSession(opId, definition.id, definition.revision, approved);
      if (active && generation === current) { session = result; status = 'Native keychain clients and exact origins are bound. Review the effective origins/budgets below, then explicitly compare. No objects were listed or downloaded during binding.'; }
      else void releaseJobSession(result.run_id).catch(() => logError('Could not release a late read-only job binding; native expiry remains active.'));
    } catch (error) { if (active && generation === current) failure = error; }
    finally { if (active && generation === current) { busy = null; approved = false; } }
  }
  async function compare() {
    if (busy || session?.phase !== 'bound') return;
    const id = session.run_id; const current = ++generation; busy = 'scanning'; failure = null; status = ''; rows = [];
    resetSelection();
    session = { ...session, phase: 'scanning', outcome: null, summary: null, manifest_id: null, progress: { scanned: 0, matched: 0, requests: 0, bytes_read: 0, s3_bytes_read: 0, elapsed_ms: 0 } };
    progress = session.progress;
    try {
      const result = await scanJobSession(id, event => { if (active && generation === current) progress = event; });
      if (active && generation === current) { session = result; progress = result.progress; if (result.phase === 'complete') status = 'Complete native comparison retained. This is read-only, not an execution approval. Destination-only entries will not be deleted.'; }
    } catch (error) { if (active && generation === current) { failure = error; session = null; status = 'Comparison unavailable; bind a new session before retrying.'; void releaseJobSession(id).catch(() => logError('Read-only job cleanup failed after comparison error; native expiry remains active.')); } }
    finally { if (active && generation === current) busy = null; }
    if (active && generation === current && session?.phase === 'complete') await more();
  }
  async function more() {
    if (busy || !session || !['complete', 'prepared'].includes(session.phase) || !session.manifest_id) return;
    const current = generation; const id = session.run_id; const manifest = session.manifest_id; busy = 'paging'; failure = null;
    try { const result = await jobManifestPage(id, manifest, rows.length); if (active && generation === current && session?.run_id === id) rows = [...rows, ...result]; }
    catch (error) { if (active && generation === current) { failure = error; rows = []; session = null; resetSelection(); status = 'Native comparison is no longer usable; bind and compare again.'; void releaseJobSession(id).catch(() => logError('Read-only job release failed after manifest invalidation; native expiry remains active.')); } }
    finally { if (active && generation === current) busy = null; }
  }
  async function refresh() {
    if (busy || !session) return;
    const id = session.run_id; const current = generation; busy = 'refreshing'; failure = null;
    try { const result = await jobSessionStatus(id); if (active && generation === current) { session = result; if (!['complete', 'prepared'].includes(result.phase)) { rows = []; resetSelection(); status = ''; } } }
    catch (error) { if (active && generation === current) failure = error; }
    finally { if (active && generation === current) busy = null; }
  }
  async function release() {
    const handle = session?.run_id ?? opId;
    if (!handle) return;
    const current = ++generation;
    failure = null; busy = null; rows = []; approved = false;
    resetSelection();
    try {
      await releaseJobSession(handle);
      if (active && generation === current) {
        status = 'Cancellation/release requested. Native requests already sent may finish; OS keychain prompts/local filesystem calls are cooperative. A new binding cannot overlap still-running work.';
        session = null; progress = null; opId = '';
      }
    } catch (error) { if (active && generation === current) failure = error; }
  }
  async function prepare() {
    if (busy || session?.phase !== 'complete' || !session.manifest_id || !snapshotApproved || !selectedIds.size) return;
    if (!Number.isInteger(snapshotMiB) || snapshotMiB < 1 || snapshotMiB > 16384) { failure = new Error('Choose a whole snapshot disk budget of 1–16,384 MiB.'); return; }
    const id = session.run_id; const manifest = session.manifest_id; const current = ++generation;
    busy = 'preparing'; failure = null; status = ''; progress = null;
    try {
      const result = await prepareJobPreview(id, manifest, [...selectedIds], previewPolicy, snapshotMiB * 1024 * 1024, snapshotApproved, event => { if (active && generation === current) progress = event; });
      if (active && generation === current) {
        session = result; progress = result.progress;
        if (result.phase === 'prepared') status = 'Checked payload snapshots retained natively. No upload, execution approval or scheduling is available.';
        else if (result.phase === 'stopped') { rows = []; resetSelection(); }
      }
    } catch (error) { if (active && generation === current) failure = error; }
    finally { if (active && generation === current) { busy = null; snapshotApproved = false; } }
  }
  const differenceCount = $derived(session?.summary ? session.summary.new + session.summary.modified + session.summary.destination_only : 0);
</script>

<section aria-label="Read-only native job comparison">
  <h3>Read-only comparison — saved revision {definition.revision}</h3>
  <p>This uses the saved definition above, not unsaved editor fields or the current Sync panes. Nothing connects or scans automatically. Only manual-key profiles are eligible; no SSO sign-in or credential-process command is run.</p>
  <p>Binding explicitly reads native keychain entries and sends at most two logical HeadBucket requests. Native clients are private, origin-guarded and read-only; shared AWS config/endpoint defaults are not silently adopted. AWS routing/custom-CA overrides and unsupported acceleration/Express settings fail closed.</p>
  <p>Binding first records a durable local read-only attempt intent. Native history contains allowlisted IDs, revisions, times, policies and stage counters, not names, roots, credentials or approvals. Journal/storage failures and unresolved review pauses block new job work; ordinary workflows remain available.</p>
  <label><input type="checkbox" bind:checked={approved} disabled={!!busy || (!!session && session.phase !== 'stopped')} /> I reviewed the saved source → destination and approve keychain access plus read-only connection validation. This does not approve scanning, uploads or deletion.</label>
  <div class="actions">
    <button type="button" onclick={bind} disabled={!approved || !!busy || (!!session && session.phase !== 'stopped')}>Bind native read-only session</button>
    <button type="button" onclick={compare} disabled={!!busy || session?.phase !== 'bound'}>Compare saved job (SHA-256)</button>
    <button type="button" onclick={release} disabled={!session && !opId}>Cancel / release session</button>
    <button type="button" onclick={refresh} disabled={!!busy || !session}>Revalidate session (no network)</button>
  </div>
  {#if busy}<p role="status">{busy === 'binding' ? 'Waiting for native keychain / read-only bucket validation…' : busy === 'scanning' ? 'Comparing pinned roots and reading included contents…' : busy === 'preparing' ? 'Rechecking destinations and retaining checked source snapshots…' : 'Reading native comparison state…'}</p>{/if}
  {#if session}
    <p>Native phase: {session.phase}; lease remaining at last check: {session.expires_seconds}s (non-sliding). Effective destination origin: {session.destination_namespace ?? 'released'}. Source: {session.source_namespace ?? (session.phase !== 'stopped' && definition.source.kind === 'local' ? 'retained local directory capability' : 'released')}.</p>
    {#if session.phase === 'bound'}<p>Bucket validation used {session.progress.requests} logical S3 calls. Comparison budgets and counters start separately when you click Compare.</p>{/if}
    {#if session.limits}<p>Effective shared comparison budgets: {session.limits.max_objects.toLocaleString()} entries, {session.limits.max_requests.toLocaleString()} logical S3 calls, {session.limits.max_seconds}s. Job caps also bound metadata and differences. SDK reads may make one bounded retry, including the existing plain-object SSE-C fallback.</p>{/if}
  {/if}
  <p>SHA-256 reads all included contents on both sides; S3 requests and download/egress charges may apply. Maximum one native session across cooperating processes, 100,000 scanned entries, 32 MiB estimated input metadata, 10,000 differences. A stopped scan establishes neither equality nor a usable manifest. No job execution, automatic retry/resume or scheduling is enabled.</p>
  {#if progress}<ScanStatus {progress} outcome={busy === 'preparing' ? undefined : failure && !busy ? 'failed' : session?.outcome ?? undefined} message={session?.message ?? undefined} />{/if}
  {#if session?.summary}<p>New: {session.summary.new}; modified: {session.summary.modified}; destination only (informational): {session.summary.destination_only}; content matches: {session.summary.same}. Displayed {rows.length} of {differenceCount} differences.</p>{/if}
  {#if rows.length}
    <div class="rows"><table><thead><tr><th>Prepare</th><th>Relative path</th><th>Status</th><th>Source bytes</th><th>Destination bytes</th></tr></thead><tbody>
      {#each rows as row (row.id)}<tr><td><input type="checkbox" aria-label={`Select ${row.relative_path} for checked preparation`} checked={selectedIds.has(row.id)} disabled={!!busy || session?.phase !== 'complete' || row.status === 'destination_only'} onchange={() => toggleEntry(row.id)} /></td><td>{row.relative_path}</td><td>{row.status}</td><td>{row.source_size ?? '—'}</td><td>{row.destination_size ?? '—'}</td></tr>{/each}
    </tbody></table></div>
  {/if}
  {#if session && ['complete', 'prepared'].includes(session.phase) && rows.length < differenceCount}<button type="button" onclick={more} disabled={!!busy}>Load next 100 native differences</button>{/if}
  {#if session?.phase === 'complete'}
    <fieldset disabled={!!busy}>
      <legend>Prepare checked payloads — no uploads</legend>
      <p>Select explicit rows above (loaded rows only). {selectedIds.size} selected; at most 128 actionable files per preview. Destination-only rows cannot be selected. Preparation is single-shot after native admission; failures/cancellation require a new bind and comparison.</p>
      <label>Future guarded policy <select bind:value={previewPolicy} onchange={() => { snapshotApproved = false; }}><option value="create_only">Create only (reject existing)</option><option value="skip_existing">Skip existing (report skipped)</option><option value="replace_unchanged">Replace unchanged (no execution approval)</option></select></label>
      <label>Maximum snapshot disk MiB <input type="number" min="1" max="16384" step="1" bind:value={snapshotMiB} oninput={() => { snapshotApproved = false; }} /></label>
      <p>Preparation rechecks included destinations (HEAD plus conditional SHA-256 reads for replacements), then reads/downloads source contents into private, unlinked temporary files. Plaintext is retained even for SSE-C sources; temporary disk, read requests and egress costs apply. The disk budget counts payload bytes, not filesystem overhead, and does not reserve free space. The existing session deadline does not slide. No payload is uploaded or deleted.</p>
      <label><input type="checkbox" bind:checked={snapshotApproved} /> I reviewed the selected saved-job entries, policy and disk budget, and approve additional reads/downloads and temporary plaintext snapshots only.</label>
      <button type="button" onclick={prepare} disabled={!snapshotApproved || !selectedIds.size}>Prepare checked preview (no uploads)</button>
    </fieldset>
  {/if}
  {#if session?.preview}
    <h3>Native checked preview — not executable</h3>
    <p>Saved job {session.preview.job_id}, revision {session.preview.revision}; policy {session.preview.policy}. {session.preview.files} payload snapshots, {session.preview.bytes.toLocaleString()} bytes; {session.preview.skipped} selected existing files skipped. Remaining lease at last check: {session.preview.expires_seconds}s. SHA-256 and verification cannot be disabled. Release the session to discard snapshots; native expiry also releases them.</p>
    <div class="rows"><table><thead><tr><th>Relative source</th><th>Exact destination key</th><th>Bytes</th><th>Condition</th></tr></thead><tbody>{#each session.preview.entries as entry (entry.entry_id)}<tr><td>{entry.relative_path}</td><td>{entry.destination_key}</td><td>{entry.size}</td><td>{entry.replaces ? 'Compared opaque ETag' : 'Must be missing'}</td></tr>{/each}</tbody></table></div>
    {#if session.preview.entries_truncated}<p>Showing the first 50 prepared mappings; all selected mappings remain native and must be reviewed again before any future execution feature.</p>{/if}
    <p>These snapshots are not a transaction or metadata migration. Destination content may change after preparation. Read-only stages/outcomes are journaled, but no executable intent, execution API or approval token was created. Guarded execution remains deferred.</p>
  {/if}
  {#if failure}<ErrorNotice error={failure} context="Native job comparison" />{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>

<style>
  section { border: 1px solid var(--border-subtle); padding: 10px; margin: 10px 0; min-width: 0; }
  h3, label { font-size: 12px; } p { font-size: 12px; color: var(--text-secondary); overflow-wrap: anywhere; line-height: 1.5; }
  .actions { display: flex; gap: 6px; flex-wrap: wrap; } button { font: inherit; font-size: 12px; padding: 4px 8px; } button:disabled { opacity: 0.5; }
  .rows { max-height: 240px; overflow: auto; } table { width: 100%; font-size: 12px; } th { text-align: left; } td { overflow-wrap: anywhere; }
</style>
