import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const source = file => readFileSync(new URL(`../src/lib/components/${file}`, import.meta.url), 'utf8');
const deferred = () => { let resolve; let reject; const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; }); return { promise, resolve, reject }; };
const preset = (name = 'Content check') => ({ id: crypto.randomUUID(), name, compare_mode: 'sha256', exclude_patterns: [] });
let sequence = 0;

function imports(code, dependencies) {
  return code.replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
    const resolved = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(resolved, `Unmapped import ${name}`); return `from ${JSON.stringify(resolved)}`;
  });
}

async function scriptFixture(file, fixture, exports, dependencies = {}) {
  const key = `presetsFixture${sequence++}`; globalThis[key] = fixture;
  const mock = body => data(body.replaceAll('FIXTURE', `globalThis[${JSON.stringify(key)}]`));
  const shared = {
    'svelte': mock('export const onMount=callback=>FIXTURE.mounts.push(callback);export const onDestroy=callback=>FIXTURE.destroys.push(callback);'),
    '#lib/services/comparison-presets.js': mock('export const loadComparisonPresets=(...args)=>FIXTURE.load(...args);export const saveComparisonPresets=(...args)=>FIXTURE.save(...args);'),
    '#lib/utils/comparison-options.js': new URL('../src/lib/utils/comparison-options.ts', import.meta.url).href,
    '#lib/utils/scan.js': new URL('../src/lib/utils/scan.ts', import.meta.url).href,
    '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
    '#lib/services/tauri.js': mock('export const syncDiff=(...args)=>FIXTURE.scan(...args);export const cancelSync=(...args)=>FIXTURE.cancel(...args);'),
    '#lib/state/app.svelte.js': mock('export const appState=FIXTURE.app;'),
  };
  fixture.mounts = []; fixture.destroys = [];
  let script = source(file).match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
  script = script.replaceAll('$props()', `globalThis[${JSON.stringify(key)}].props`)
    .replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm, '');
  script += `\nexport const harness = { ${exports} };`;
  const code = compileModule(stripTypeScriptTypes(script), { generate: 'client', filename: file + '.js' }).js.code;
  const { harness } = await import(data(imports(code, { ...shared, ...dependencies })));
  return { harness, cleanup() { for (const destroy of fixture.destroys) destroy(); delete globalThis[key]; } };
}

const presetExports = `refresh, apply, mutate, handleKeydown,
  choose(id) { selectedId = id; name = report?.presets.find(entry => entry.id === id)?.name ?? ''; confirmDelete = false; },
  setName(value) { name = value; }, confirm() { confirmDelete = true; },
  get state() { return { report, selectedId, name, ready, busy, failure, status }; }`;

test('actual preset controller never applies, scans or transfers on load/save/update/delete', async () => {
  let disk = { revision: 'initial', presets: [preset()] }; let applies = 0; const writes = [];
  const fixture = { props: { mode: 'size_mtime', excludeText: '*.tmp, nested/**', onApply: () => applies++ }, load: async () => structuredClone(disk), save: async (revision, presets) => { assert.equal(revision, disk.revision); writes.push(structuredClone(presets)); disk = { revision: 'r' + writes.length, presets: structuredClone(presets) }; return structuredClone(disk); } };
  const { harness: h, cleanup } = await scriptFixture('ComparisonPresets.svelte', fixture, presetExports);
  try {
    await h.refresh(); assert.equal(applies, 0); assert.equal(h.state.selectedId, '');
    let prevented = 0;
    h.handleKeydown({ key: 'Enter', target: { matches: selector => selector === 'input[type="text"]' }, preventDefault() { prevented++; }, stopPropagation() {} });
    assert.equal(prevented, 1); assert.equal(applies, 0); // No implicit parent form submission.
    h.choose(disk.presets[0].id); assert.equal(applies, 0); // Selecting is not applying.
    h.apply(); assert.equal(applies, 1);
    h.setName('Metadata review'); await h.mutate('create'); assert.equal(applies, 1);
    assert.equal(disk.presets[1].compare_mode, 'size_mtime'); assert.deepEqual(disk.presets[1].exclude_patterns, ['*.tmp', 'nested/**']);
    const created = h.state.selectedId; h.setName('Renamed'); await h.mutate('update');
    assert.equal(disk.presets[1].id, created); assert.equal(disk.presets[1].name, 'Renamed');
    await h.mutate('delete'); assert.equal(writes.length, 2); // Confirmation is required.
    h.confirm(); await h.mutate('delete'); assert.equal(disk.presets.length, 1); assert.equal(applies, 1);
    assert.deepEqual(disk.presets[0].exclude_patterns, []); // No implicit exclusions.
  } finally { cleanup(); }
});

test('actual preset controller blocks writes after load errors and preserves committed state on save failure', async () => {
  const committed = { revision: 'old', presets: [preset()] }; const pending = deferred(); let writes = 0; let failLoad = true;
  const fixture = { props: { mode: 'sha256', excludeText: '', onApply() {} }, load: async () => { if (failLoad) throw new Error('Invalid preset file'); return committed; }, save: async () => { writes++; return pending.promise; } };
  const { harness: h, cleanup } = await scriptFixture('ComparisonPresets.svelte', fixture, presetExports);
  try {
    await h.refresh(); assert.equal(h.state.ready, false); assert.match(String(h.state.failure), /Invalid/);
    h.setName('New'); await h.mutate('create'); assert.equal(writes, 0);
    failLoad = false; await h.refresh(); h.setName('New');
    const save = h.mutate('create'); assert.equal(h.state.busy, true); await h.mutate('create'); assert.equal(writes, 1);
    pending.reject(new Error('Revision conflict; refresh')); await save;
    assert.equal(h.state.report.revision, 'old'); assert.equal(h.state.report.presets.length, 1); assert.match(String(h.state.failure), /Revision conflict/);
  } finally { cleanup(); }
});

test('actual Sync controller requires explicit fresh complete comparison after preset application and ignores late events', async () => {
  const scans = []; const cancellations = []; const syncs = [];
  const fixture = {
    props: { sourceBackend: 's3', sourcePath: 's3://left/root/', sourceS3Id: 'linode-source', destBackend: 's3', destPath: 's3://right/dest/', destS3Id: 'akamai-dest', onSync: entries => syncs.push(entries), onClose() {} },
    app: { syncExcludePatterns: '*.tmp', persistConfig: async () => {} },
    scan: async (...args) => { scans.push(args); }, cancel: async id => { cancellations.push(id); },
  };
  const { harness: h, cleanup } = await scriptFixture('SyncDialog.svelte', fixture, `startScan, applyPreset, handleSync, handleKeydown,
    get state() { return { scanning, scanComplete, outcome, compareMode, excludeText, entries: [...allEntries], selected: [...selectedPaths] }; }`);
  const entry = { type: 'Entry', relative_path: 'nested/file', status: 'new', source_size: 1, dest_size: 0 };
  try {
    for (const mount of fixture.mounts) mount(); assert.equal(scans.length, 0); h.handleSync(); assert.equal(syncs.length, 0);
    h.handleKeydown({ key: 'Enter', target: { closest: selector => selector === '[data-comparison-presets]' ? null : {} }, preventDefault() { throw new Error('Focused Compare button must keep its native activation'); } });
    h.startScan(); assert.equal(scans.length, 1); const oldEvent = scans[0][9];
    oldEvent(entry); oldEvent({ type: 'Done', new_count: 1, modified: 0, deleted: 0 }); assert.equal(h.state.scanComplete, true);
    h.applyPreset('sha256', []); assert.equal(scans.length, 1); assert.equal(cancellations.length, 1); assert.equal(h.state.scanComplete, false); assert.equal(h.state.selected.length, 0);
    oldEvent(entry); oldEvent({ type: 'Done' }); h.handleSync(); assert.equal(syncs.length, 0); assert.equal(h.state.scanComplete, false);
    h.startScan(); const args = scans[1]; assert.deepEqual(args.slice(1, 7), ['s3', 's3://left/root/', 'linode-source', 's3', 's3://right/dest/', 'akamai-dest']);
    assert.deepEqual(args[7], []); assert.equal(args[8], 'sha256');
    args[9](entry); args[9]({ type: 'ScanStopped', outcome: 'limited', message: 'Budget exhausted', progress: {} }); h.handleSync(); assert.equal(syncs.length, 0);
    h.startScan(); const finalEvent = scans[2][9]; finalEvent(entry); finalEvent({ type: 'Done', new_count: 1, modified: 0, deleted: 0 });
    const key = { key: 'Enter', target: { closest: () => true }, preventDefault() { throw new Error('Preset key must not trigger Sync'); } }; h.handleKeydown(key); assert.equal(syncs.length, 0);
    h.handleSync(); assert.equal(syncs.length, 1); assert.equal(syncs[0][0].relative_path, 'nested/file');
  } finally { cleanup(); }
});

test('preset service IPC is allowlisted and has no connection or execution calls', async () => {
  const calls = []; globalThis.__comparisonPresetRpc = async (...args) => { calls.push(args); return {}; };
  try {
    const text = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/comparison-presets.ts', import.meta.url), 'utf8'));
    const { loadComparisonPresets, saveComparisonPresets } = await import(data(imports(text, { '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__comparisonPresetRpc(...args);') })));
    await loadComparisonPresets(); const entry = { ...preset(), secretKey: 'DO_NOT_SEND', endpoint: 'PRIVATE_ENDPOINT', sourcePath: 'PRIVATE_PATH', write_policy: 'overwrite', approval: 'OLD_APPROVAL' };
    await saveComparisonPresets('revision', [entry]);
    assert.deepEqual(calls.map(call => call[0]), ['comparison_presets_load', 'comparison_presets_save']);
    const args = calls[1][1]; assert.equal(args.expectedRevision, 'revision'); assert.deepEqual(Object.keys(args.presets[0]).sort(), ['compare_mode', 'exclude_patterns', 'id', 'name']);
    assert.ok(!JSON.stringify(args).includes('PRIVATE')); assert.ok(!JSON.stringify(args).includes('DO_NOT_SEND'));
  } finally { delete globalThis.__comparisonPresetRpc; }
});

test('both actual dialogs render shared options-only presets and explicit roots', async () => {
  const dependencies = {
    '#lib/utils/comparison-options.js': new URL('../src/lib/utils/comparison-options.ts', import.meta.url).href,
    '#lib/utils/scan.js': new URL('../src/lib/utils/scan.ts', import.meta.url).href,
    '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
    '#lib/utils/format.js': new URL('../src/lib/utils/format.ts', import.meta.url).href,
    '#lib/services/comparison-presets.js': data('export const loadComparisonPresets=async()=>{throw new Error("No automatic SSR IO");};export const saveComparisonPresets=async()=>{throw new Error("No auto save");};'),
    '#lib/state/app.svelte.js': data('export const appState={syncExcludePatterns:"*.tmp",s3ScanLimits:{max_objects:100000,max_requests:1000,max_seconds:300}};'),
    '#lib/services/tauri.js': data('export const syncDiff=async()=>{throw new Error("No automatic scan");};export const cancelSync=async()=>{};'),
  };
  const component = file => data(imports(compile(source(file), { generate: 'server', filename: file }).js.code, dependencies));
  dependencies['./ErrorNotice.svelte'] = component('ErrorNotice.svelte'); dependencies['./ComparisonPresets.svelte'] = component('ComparisonPresets.svelte'); dependencies['./ScanStatus.svelte'] = component('ScanStatus.svelte');
  const { default: Compare } = await import(component('CompareDialog.svelte'));
  const comparison = render(Compare, { props: { leftPath: 's3://left/root/', rightPath: 's3://right/dest/', leftBackend: 's3', rightBackend: 's3', onCompare() {}, onClose() {} } }).body;
  assert.ok(comparison.includes('data-comparison-presets')); assert.ok(comparison.includes('s3://left/root/')); assert.ok(comparison.includes('s3://right/dest/'));
  const { default: Sync } = await import(component('SyncDialog.svelte'));
  const sync = render(Sync, { props: { sourceBackend: 's3', sourcePath: 's3://left/root/', sourceS3Id: 'left', destBackend: 's3', destPath: 's3://right/dest/', destS3Id: 'right', onSync() {}, onClose() {} } }).body;
  assert.ok(sync.includes('data-comparison-presets')); assert.ok(sync.includes('Opening this dialog or applying a preset does not start a scan'));
  assert.match(sync, /disabled[^>]*>\s*Sync \(0\)/); assert.ok(sync.includes('No paths, connection identities, secrets'));
});
