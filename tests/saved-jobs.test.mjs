import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const source = readFileSync(new URL('../src/lib/components/SavedSyncJobs.svelte', import.meta.url), 'utf8');
const configuration = { bucket: 'bucket-b', region: 'us-ord-1', endpoint: 'https://us-ord-1.linodeobjects.com/', provider: 'linode', proxy_endpoint: null, proxy_auth: false, sse_customer_key_enabled: true, acceleration: false };
const definition = { id: crypto.randomUUID(), revision: '1', name: 'Pinned definition', source: { kind: 'local', path: '/source/root', device: '1', inode: '2' }, destination: { profile_id: 'destination', generation: crypto.randomUUID(), configuration, prefix: 'target//literal/../' }, exclude_patterns: [], compare_mode: 'sha256', operation: 'copy_update_only' };
const report = () => ({ revision: 'r1', configuration_revision: 'c1', execution_available: false, jobs: [{ definition: structuredClone(definition), status: 'definition_valid', reason: 'Configuration only; not executable' }], profiles: [{ id: 'destination', name: 'Linode/Akamai', configuration, unavailable: null }] });
const deferred = () => { let resolve; let reject; const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; }); return { resolve, reject, promise }; };
let sequence = 0;
function imports(code, dependencies) {
  return code.replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
    const resolved = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(resolved, `Unmapped saved-job import: ${name}`); return `from ${JSON.stringify(resolved)}`;
  });
}
async function fixture(load, save, remove) {
  const key = `savedJobsFixture${sequence++}`;
  const state = { props: { defaultLocalPath: '/current/pane/source' }, load, save, remove };
  globalThis[key] = state;
  const dependencies = {
    'svelte': data('export const onMount=()=>{};export const untrack=callback=>callback();'),
    '#lib/services/saved-jobs.js': data(`export const listSavedJobs=(...args)=>globalThis.${key}.load(...args);export const saveJobDefinition=(...args)=>globalThis.${key}.save(...args);export const deleteJobDefinition=(...args)=>globalThis.${key}.remove(...args);`),
    '#lib/utils/comparison-options.js': new URL('../src/lib/utils/comparison-options.ts', import.meta.url).href,
  };
  let script = source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replaceAll('$props()', `globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm, '');
  script += `\nexport const harness={refresh, choose, save, remove, changed,
    name(value){ name=value; changed(); }, approve(){ acknowledged=true; }, confirm(){ confirmDelete=true; }, close(){active=false;},
    get state(){ return {report,selectedId,name,sourceKind,localPath,destinationProfile,destinationPrefix,excludeText,acknowledged,busy,ready,failure,status}; }
  };`;
  const code = compileModule(stripTypeScriptTypes(script), { generate: 'client', filename: 'SavedSyncJobs.svelte.js' }).js.code;
  const { harness } = await import(data(imports(code, dependencies)));
  return { h: harness, cleanup: () => { harness.close(); delete globalThis[key]; } };
}

test('actual job editor loads and explicitly saves/rebinds configuration only, retaining exact direction and resetting approval', async () => {
  const saved = []; const removed = []; let loads = 0; let disk = report();
  const { h, cleanup } = await fixture(async () => { loads++; return structuredClone(disk); }, async request => {
    saved.push(structuredClone(request)); const job = { ...structuredClone(definition), id: request.id ?? crypto.randomUUID(), name: request.name, revision: request.id ? '2' : '1', exclude_patterns: request.exclude_patterns };
    disk = { ...disk, revision: 'r' + (saved.length + 1), jobs: [{ definition: job, status: 'definition_valid', reason: 'Not executable' }] }; return structuredClone(disk);
  }, async (...args) => { removed.push(args); disk = { ...disk, revision: 'deleted', jobs: [] }; return structuredClone(disk); });
  try {
    await h.refresh(); assert.equal(loads, 1); assert.equal(saved.length, 0); assert.equal(h.state.selectedId, '');
    assert.equal(h.state.localPath, '/current/pane/source');
    h.choose(definition.id); assert.equal(saved.length, 0); assert.equal(h.state.localPath, '/source/root');
    assert.equal(h.state.destinationPrefix, 'target//literal/../'); assert.equal(h.state.excludeText, '');
    await h.save(false); assert.equal(saved.length, 0); // Separate acknowledgement required.
    h.name('Explicitly rebound'); h.approve(); await h.save(false);
    assert.equal(saved.length, 1); assert.equal(saved[0].id, definition.id); assert.equal(saved[0].expected_configuration_revision, 'c1');
    assert.deepEqual(saved[0].source, { kind: 'local', path: '/source/root' }); assert.equal(saved[0].destination_profile_id, 'destination');
    assert.deepEqual(saved[0].exclude_patterns, []); assert.equal(h.state.acknowledged, false); assert.equal(h.state.report.execution_available, false);
    h.approve(); h.changed(); assert.equal(h.state.acknowledged, false);
    await h.remove(); assert.equal(removed.length, 0); h.confirm(); await h.remove(); assert.equal(removed.length, 1); assert.equal(h.state.report.jobs.length, 0);
  } finally { cleanup(); }
});

test('actual job editor blocks changes after load errors and preserves last confirmed report on conflicts', async () => {
  let failLoad = true; let writes = 0; const pending = deferred();
  const { h, cleanup } = await fixture(async () => { if (failLoad) throw new Error('Configuration unavailable'); return report(); }, async () => { writes++; return pending.promise; }, async () => { throw new Error('Delete failure'); });
  try {
    await h.refresh(); assert.equal(h.state.ready, false); h.approve(); await h.save(true); assert.equal(writes, 0); assert.match(String(h.state.failure), /Configuration unavailable/);
    failLoad = false; await h.refresh(); h.choose(definition.id); h.approve(); const first = h.save(false); await h.save(false); assert.equal(writes, 1);
    pending.reject(new Error('Profile changed; refresh and review')); await first;
    assert.equal(h.state.report.revision, 'r1'); assert.equal(h.state.report.jobs[0].definition.revision, '1'); assert.equal(h.state.acknowledged, false); assert.match(String(h.state.failure), /Profile changed/);
  } finally { cleanup(); }
});

test('late save completions cannot update a closed editor or launch another workflow', async () => {
  const pending = deferred(); const { h, cleanup } = await fixture(async () => report(), async () => pending.promise, async () => report());
  try {
    await h.refresh(); h.choose(definition.id); h.approve(); const saving = h.save(false); h.close();
    pending.resolve({ ...report(), revision: 'late' }); await saving; assert.equal(h.state.report.revision, 'r1');
  } finally { cleanup(); }
});

test('job service allowlists definition requests and exposes no connect, scan, execution or credential IPC', async () => {
  const calls = []; globalThis.__savedJobRpc = async (...args) => { calls.push(args); return {}; };
  try {
    const script = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/saved-jobs.ts', import.meta.url), 'utf8'));
    const service = await import(data(imports(script, { '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__savedJobRpc(...args);') })));
    await service.listSavedJobs();
    await service.saveJobDefinition({ expected_revision: 'r', expected_configuration_revision: 'c', id: null, name: 'Definition', source: { kind: 's3', profile_id: 'source', prefix: 'from/', secret_key: 'DO_NOT_SEND', configuration: { endpoint: 'https://attacker.example' } }, destination_profile_id: 'destination', destination_prefix: 'to/', exclude_patterns: [], acknowledged: true, approval: 'OLD_APPROVAL', connectionId: 'TEMPORARY_ID', secretKey: 'DO_NOT_SEND' });
    await service.deleteJobDefinition('r', 'id', true);
    assert.deepEqual(calls.map(call => call[0]), ['saved_jobs_list', 'saved_jobs_save', 'saved_jobs_delete']);
    const encoded = JSON.stringify(calls); for (const value of ['DO_NOT_SEND', 'attacker.example', 'OLD_APPROVAL', 'TEMPORARY_ID']) assert.ok(!encoded.includes(value));
    assert.deepEqual(calls[1][1].request.source, { kind: 's3', profile_id: 'source', prefix: 'from/' });
  } finally { delete globalThis.__savedJobRpc; }
});

test('actual job editor renders configuration-only warnings and permanently disabled execution', async () => {
  const errorSource = readFileSync(new URL('../src/lib/components/ErrorNotice.svelte', import.meta.url), 'utf8');
  const notice = data(imports(compile(errorSource, { generate: 'server', filename: 'ErrorNotice.svelte' }).js.code, { '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href }));
  const code = compile(source, { generate: 'server', filename: 'SavedSyncJobs.svelte' }).js.code;
  const dependencies = { './ErrorNotice.svelte': notice, './JobComparisonSession.svelte': data('export default ()=>{};'), '#lib/services/saved-jobs.js': data('export const listSavedJobs=async()=>{throw new Error("SSR must not access storage");};export const saveJobDefinition=async()=>{};export const deleteJobDefinition=async()=>{};'), '#lib/utils/comparison-options.js': new URL('../src/lib/utils/comparison-options.ts', import.meta.url).href };
  const { default: Component } = await import(data(imports(code, dependencies)));
  const html = render(Component, { props: { defaultLocalPath: '/current/source' } }).body;
  assert.ok(html.includes('data-saved-jobs')); assert.ok(html.includes('separate from the current Sync operation')); assert.match(html, /disabled[^>]*>Run job — not implemented/);
  assert.ok(html.includes('It does not read keychain secrets')); assert.ok(html.includes('private plaintext file')); assert.ok(html.includes('/current/source'));
});

test('actual app observes ignored configuration-save failures while preserving rejection and ignoring stale errors', async () => {
  const pending = []; globalThis.__jobConfigSave = () => { const next = deferred(); pending.push(next); return next.promise; };
  try {
    const text = stripTypeScriptTypes(readFileSync(new URL('../src/lib/state/app.svelte.ts', import.meta.url), 'utf8'));
    const dependencies = {
      '#lib/services/config.js': data('export const saveConfig=(...args)=>globalThis.__jobConfigSave(...args);'),
      '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
      '#lib/utils/scan.js': new URL('../src/lib/utils/scan.ts', import.meta.url).href,
      '#lib/utils/columns.js': new URL('../src/lib/utils/columns.ts', import.meta.url).href,
      '#lib/services/s3.js': data('export const s3SetBandwidthLimit=async()=>{};export const s3SetMultipartConfig=async()=>{};export const s3SetScanLimits=async()=>{};'),
    };
    for (const [file, name, value] of [['sidebar', 'sidebarState', { favorites: [] }], ['workspaces', 'workspacesState', { workspaces: [] }], ['connections', 'connectionsState', { profiles: [] }], ['s3bookmarks', 's3BookmarksState', { bookmarks: [] }], ['sftpbookmarks', 'sftpBookmarksState', { bookmarks: [] }], ['transfers', 'transfersState', { bandwidthLimit: 0, maxConcurrent: 2 }], ['status', 'statusState', {}]]) dependencies[`#lib/state/${file}.svelte.js`] = data(`export const ${name}=${JSON.stringify(value)};`);
    const compiled = compileModule(text, { generate: 'client', filename: 'app.svelte.js' }).js.code;
    const { appState } = await import(data(imports(compiled, dependencies)));
    const save = appState.persistConfig(); pending[0].reject(new Error('Native identity storage unavailable'));
    await assert.rejects(save, /identity storage/); assert.match(appState.configSaveError, /identity storage/);
    const old = appState.persistConfig(); const latest = appState.persistConfig(); pending[2].resolve(); await latest; assert.equal(appState.configSaveError, '');
    pending[1].reject(new Error('Stale failure')); await assert.rejects(old); assert.equal(appState.configSaveError, '');
    appState.persistConfig(); pending[3].reject(new Error('Unawaited settings failure')); await Promise.resolve(); await Promise.resolve(); assert.match(appState.configSaveError, /Unawaited/);
  } finally { delete globalThis.__jobConfigSave; }
});

test('alert-only confirmations keep long identity/credential errors collapsed and copyable', async () => {
  const errorSource = readFileSync(new URL('../src/lib/components/ErrorNotice.svelte', import.meta.url), 'utf8');
  const notice = data(imports(compile(errorSource, { generate: 'server', filename: 'ErrorNotice.svelte' }).js.code, { '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href }));
  const confirmSource = readFileSync(new URL('../src/lib/components/ConfirmDialog.svelte', import.meta.url), 'utf8');
  const { default: Confirm } = await import(data(imports(compile(confirmSource, { generate: 'server', filename: 'ConfirmDialog.svelte' }).js.code, { './ErrorNotice.svelte': notice })));
  const html = render(Confirm, { props: { alertOnly: true, message: 'Credential cleanup failed\nReview partial cleanup before retrying\nSafe diagnostic details', onConfirm() {}, onCancel() {} } }).body;
  assert.ok(html.includes('Safe diagnostic details')); assert.ok(html.includes('<details')); assert.ok(!html.includes('<details open')); assert.ok(html.includes('Copy'));
});

test('actual connection deletion retains the profile and reports partial cleanup when keychain/identity tracking fails', async () => {
  const deleted = []; const removed = []; const alerts = []; let confirm; let failSse = true;
  globalThis.__jobDeleteFixture = {
    app: { showConfirm(_message, callback) { confirm = callback; }, closeModal() {}, showAlert(message) { alerts.push(message); }, showConnectionManager() {} },
    connections: { deleteSecret: async id => { deleted.push(id); if (failSse && id.endsWith(':sse-c')) throw new Error('Identity storage unavailable'); }, removeProfile: id => removed.push(id) },
  };
  try {
    let script = readFileSync(new URL('../src/lib/components/ConnectionManager.svelte', import.meta.url), 'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1]
      .replaceAll('$props()', '({onClose(){}})').replace(/^\s*import \w+ from '[^']+\.svelte';/gm, '')
      .replace(/\$effect\(\(\) => \{\s*if \(overlayEl\) overlayEl\.focus\(\);\s*\}\);/, '');
    script += '\nexport const deleteConnection=handleDelete;';
    const dependencies = {
      'svelte': data('export const untrack=callback=>callback();'),
      '#lib/utils/aws-sso.js': new URL('../src/lib/utils/aws-sso.ts', import.meta.url).href,
      '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
      '#lib/state/app.svelte.js': data('export const appState=globalThis.__jobDeleteFixture.app;'),
      '#lib/state/connections.svelte.js': data('export const connectionsState=globalThis.__jobDeleteFixture.connections;'),
      '#lib/state/panels.svelte.js': data('export const panels={};'),
      '#lib/data/s3-providers.js': data('export const resolveCapabilities=()=>({});export const getProviderIcon=()=>"";'),
      '#lib/services/log.js': data('export const error=()=>{};'),
      '#lib/services/s3.js': data('export const oidcRefresh=async()=>{};'),
    };
    const compiled = compileModule(stripTypeScriptTypes(script), { generate: 'client', filename: 'ConnectionManager.svelte.js' }).js.code;
    const { deleteConnection } = await import(data(imports(compiled, dependencies)));
    const profile = { id: 'saved-profile', name: 'Linode', type: 's3', credentialType: 'keychain', sseCustomerKeyEnabled: true };
    deleteConnection(profile); await confirm(); assert.deepEqual(deleted, ['saved-profile', 'saved-profile:sse-c']); assert.deepEqual(removed, []);
    assert.match(alerts[0], /Some keychain entries may already have been removed/); assert.match(alerts[0], /Identity storage unavailable/);
    failSse = false; deleteConnection(profile); await confirm(); assert.deepEqual(removed, ['saved-profile']);
  } finally { delete globalThis.__jobDeleteFixture; }
});
