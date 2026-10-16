import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const source = file => readFileSync(new URL(`../src/lib/components/${file}`, import.meta.url), 'utf8');
const report = () => ({ revision: 'native-revision', storage_review_required: false, in_memory_storage_fault: false, reconciliation_deferred: false, pauses: { job: { run_id: 'abandoned-run', reason: 'interrupted' } }, records: [], execution_available: false });
const deferred = () => { let resolve; let reject; const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; }); return { promise, resolve, reject }; };
function imports(code, dependencies) {
  return code.replace(/from (['"])([^'"]+)\1/g, (_, quote, name) => {
    const target = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(target, `Unmapped journal import ${name}`); return `from ${JSON.stringify(target)}`;
  });
}
let sequence = 0;
async function controller(file, methods = {}) {
  const key = `jobJournalFixture${sequence++}`;
  const context = { props: { labels: {} }, mounts: [], calls: [], shared: { report: null, error: null }, methods: { load: async () => report(), review: async () => ({ ...report(), revision: 'new-revision', pauses: {} }), ...methods } };
  globalThis[key] = context;
  const rpc = data(`const run=(method,...args)=>{const f=globalThis.${key};f.calls.push([method,...args]);return f.methods[method](...args);};export const loadJobHistory=()=>run('load');export const reviewJobHistory=(...args)=>run('review',...args);export const reviewMultipartCleanup=(...args)=>run('cleanup',...args);`);
  let script = source(file).match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replaceAll('$props()', `globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm, '');
  script += file === 'JobHistory.svelte'
    ? '\nexport const harness={refresh,review,clear,resolveCleanup,selectCleanup(value){cleanupId=value;cleanupReviewed=false;},acknowledgeCleanup(){cleanupReviewed=true;},selectJob(value){jobId=value;acknowledged=false;},acknowledge(){acknowledged=true;},confirmRemoval(){confirmClear=true;},get state(){return{report,busy,failure,acknowledged,confirmClear,cleanupReviewed};}};'
    : '\nexport const harness={refresh};';
  const code = compileModule(stripTypeScriptTypes(script), { generate: 'client', filename: `${file}.js` }).js.code;
  const { harness } = await import(data(imports(code, {
    svelte: data(`export const onMount=callback=>globalThis.${key}.mounts.push(callback);`),
    '#lib/services/job-journal.js': rpc,
    '#lib/state/job-journal.svelte.js': data(`export const jobJournalState=globalThis.${key}.shared;`),
    '#lib/state/job-execution.svelte.js': data('export const jobExecutionState={status:null,error:null};'),
  })));
  let destroy;
  return { h: harness, ...context, mount() { destroy = context.mounts[0](); }, close() { destroy?.(); }, cleanup() { delete globalThis[key]; } };
}

test('actual history controller loads only local metadata and requires separate revision-bound review', async () => {
  const f = await controller('JobHistory.svelte');
  try {
    assert.equal(f.calls.length, 0); await f.h.refresh(); assert.deepEqual(f.calls.map(call => call[0]), ['load']);
    await f.h.review(false); assert.equal(f.calls.length, 1); f.h.selectJob('job'); f.h.acknowledge(); await f.h.review(false);
    assert.deepEqual(f.calls[1], ['review', 'native-revision', 'job', true, false]); assert.equal(f.h.state.acknowledged, false); assert.equal(f.h.state.report.execution_available, false);
    assert.ok(!f.calls.some(call => /bind|connect|scan|prepare|execute|resume|upload/.test(call[0])));
  } finally { f.close(); f.cleanup(); }
});

test('review/removal is deferred for live ownership and removal has its own acknowledgement', async () => {
  const f = await controller('JobHistory.svelte', { load: async () => ({ ...report(), reconciliation_deferred: true }) });
  try {
    await f.h.refresh(); f.h.selectJob('job'); f.h.acknowledge(); f.h.confirmRemoval(); await f.h.review(false); await f.h.clear(); assert.deepEqual(f.calls.map(call => call[0]), ['load']);
  } finally { f.cleanup(); }
  const resolved = await controller('JobHistory.svelte', { load: async () => ({ ...report(), pauses: {} }) });
  try { await resolved.h.refresh(); await resolved.h.clear(); assert.equal(resolved.calls.length, 1); resolved.h.confirmRemoval(); await resolved.h.clear(); assert.deepEqual(resolved.calls[1], ['review', 'native-revision', null, true, true]); assert.equal(resolved.h.state.confirmClear, false); } finally { resolved.cleanup(); }
});

test('stale or failed history writes retain the last confirmed report and reset acknowledgements', async () => {
  const f = await controller('JobHistory.svelte', { review: async () => { throw new Error('[JOB_JOURNAL] Revision changed; refresh before review'); } });
  try { await f.h.refresh(); f.h.selectJob('job'); f.h.acknowledge(); await f.h.review(false); assert.match(String(f.h.state.failure), /Revision changed/); assert.equal(f.h.state.report.revision, 'native-revision'); assert.equal(f.h.state.acknowledged, false); assert.equal(f.calls.length, 2); } finally { f.cleanup(); }
});

test('closed history views cannot publish late review replies or start another action', async () => {
  const pending = deferred(); const f = await controller('JobHistory.svelte', { review: async () => pending.promise });
  try { f.mount(); await Promise.resolve(); await Promise.resolve(); f.h.selectJob('job'); f.h.acknowledge(); const review = f.h.review(false); f.close(); pending.resolve({ ...report(), revision: 'late', pauses: {} }); await review; assert.equal(f.h.state.report.revision, 'native-revision'); assert.deepEqual(f.calls.map(call => call[0]), ['load', 'review']); } finally { f.cleanup(); }
});

test('startup notice invokes only native local history/reconciliation, not connection or execution', async () => {
  const f = await controller('JobJournalNotice.svelte');
  try { f.mount(); await Promise.resolve(); await Promise.resolve(); assert.deepEqual(f.calls.map(call => call[0]), ['load']); } finally { f.cleanup(); }
});

test('pending multipart evidence blocks removal and requires separate revision-bound external review',async()=>{
  const pending={...report(),pauses:{},pending_uploads:[{id:'native-evidence',run_id:'run',state:'aborting',upload_id_known:true}]};
  const f=await controller('JobHistory.svelte',{load:async()=>pending,cleanup:async()=>({...pending,pending_uploads:[],revision:'reviewed'})});
  try{await f.h.refresh();f.h.confirmRemoval();await f.h.clear();f.h.selectCleanup('native-evidence');f.h.acknowledge();await f.h.resolveCleanup();assert.equal(f.calls.length,1);f.h.acknowledgeCleanup();await f.h.resolveCleanup();assert.deepEqual(f.calls[1],['cleanup','native-revision','native-evidence',true]);assert.equal(f.h.state.cleanupReviewed,false);assert.equal(f.h.state.report.execution_available,false);}finally{f.cleanup();}
});

test('external cleanup review IPC is local-ID-only and never restores remote authority',async()=>{
  globalThis.__cleanupReviewShared={report:null,error:null};const calls=[];globalThis.__cleanupReviewRpc=async(...args)=>{calls.push(args);return{...report(),pending_uploads:[]};};
  try{const script=stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-journal.ts',import.meta.url),'utf8'));const service=await import(data(imports(script,{'@tauri-apps/api/core':data('export const invoke=(...args)=>globalThis.__cleanupReviewRpc(...args);'),'#lib/state/job-journal.svelte.js':data('export const jobJournalState=globalThis.__cleanupReviewShared;')})));await assert.rejects(service.reviewMultipartCleanup('revision','native-evidence',false));assert.equal(calls.length,0);await service.reviewMultipartCleanup('revision','native-evidence',true);assert.deepEqual(calls,[['saved_job_multipart_cleanup_review',{expectedRevision:'revision',evidenceId:'native-evidence',acknowledged:true}]]);}finally{delete globalThis.__cleanupReviewShared;delete globalThis.__cleanupReviewRpc;}
});

test('journal IPC is allowlisted and storage failures survive closed run controllers in shared state', async () => {
  const shared = { report: null, error: null }; const calls = []; globalThis.__journalShared = shared;
  globalThis.__journalRpc = async (command, args) => { calls.push([command, args]); return report(); };
  try {
    const script = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-journal.ts', import.meta.url), 'utf8'));
    const service = await import(data(imports(script, { '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__journalRpc(...args);'), '#lib/state/job-journal.svelte.js': data('export const jobJournalState=globalThis.__journalShared;') })));
    await assert.rejects(service.reviewJobHistory('revision', 'job', false)); assert.equal(calls.length, 0); await service.loadJobHistory(); await service.reviewJobHistory('native-revision', 'job', true);
    assert.deepEqual(calls.map(call => call[0]), ['saved_job_history', 'saved_job_history_review']); assert.deepEqual(calls[1][1], { expectedRevision: 'native-revision', jobId: 'job', acknowledged: true, clearHistory: false });
    service.noteJobJournalFailure('S3: AccessDenied'); assert.equal(shared.error, null); service.noteJobJournalFailure('[JOB_JOURNAL] Could not durably record outcome'); assert.match(String(shared.error), /Could not durably/);
    globalThis.__journalRpc = async () => { throw new Error('[JOB_JOURNAL] Storage unavailable'); }; await assert.rejects(service.loadJobHistory()); assert.match(String(shared.error), /Storage unavailable/);
    globalThis.__journalRpc = async () => ({ ...report(), pauses: {}, in_memory_storage_fault: true }); await service.loadJobHistory(); assert.ok(shared.error);
    globalThis.__journalRpc = async () => ({ ...report(), pauses: {} }); await service.reviewJobHistory('native-revision', null, true); assert.equal(shared.error, null);
    const pending = deferred(); globalThis.__journalRpc = async () => pending.promise;
    const oldStartupReply = service.loadJobHistory(); service.noteJobJournalFailure('[JOB_JOURNAL] Newer storage failure must remain visible');
    pending.resolve({ ...report(), pauses: {} }); await oldStartupReply; assert.match(String(shared.error), /Newer storage failure/);
  } finally { delete globalThis.__journalShared; delete globalThis.__journalRpc; }
});

test('actual startup and history UI render safety warnings and collapsed copyable diagnostics', async () => {
  const shared = { report: report(), error: '<private>\n' + 'Long storage diagnostic '.repeat(30) }; globalThis.__journalRenderState = shared;
  try {
    const dependencies = { '#lib/services/job-journal.js': data('export const loadJobHistory=async()=>{throw new Error("No SSR I/O");};export const reviewJobHistory=async()=>{};export const reviewMultipartCleanup=async()=>{};'), '#lib/state/job-journal.svelte.js': data('export const jobJournalState=globalThis.__journalRenderState;'), '#lib/state/job-execution.svelte.js': data('export const jobExecutionState={status:null,error:null};'), '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href };
    const component = file => data(imports(compile(source(file), { generate: 'server', filename: file }).js.code, dependencies));
    dependencies['./ErrorNotice.svelte'] = component('ErrorNotice.svelte');
    const { default: Notice } = await import(component('JobJournalNotice.svelte')); const html = render(Notice).body;
    assert.ok(html.includes('Ordinary workflows remain available')); assert.ok(html.includes('&lt;private>')); assert.ok(!html.includes('<private>')); assert.ok(!/<details[^>]*\bopen\b/.test(html));
    const { default: History } = await import(component('JobHistory.svelte')); const history = render(History).body; assert.ok(history.includes('never resumes or executes')); assert.ok(history.includes('not object names')); assert.ok(history.includes('Nothing reconnects'));
  } finally { delete globalThis.__journalRenderState; }
});
