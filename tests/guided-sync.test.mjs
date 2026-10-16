import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';
import { locationError, syncLocationError, s3Folder } from '../src/lib/utils/sync-workflow.ts';

const data = text => `data:text/javascript;base64,${Buffer.from(text).toString('base64')}`;
const file = name => readFileSync(new URL(`../src/${name}`, import.meta.url), 'utf8');
const deferred = () => { let resolve; let reject; const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; }); return { promise, resolve, reject }; };
function imports(text, dependencies) { return text.replace(/from (['"])([^'"]+)\1/g, (_, quote, name) => {
  const resolved = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
  assert.ok(resolved, `Unmapped guide import ${name}`); return `from ${JSON.stringify(resolved)}`;
}); }
const local = path => ({ backend:'local', path, s3Id:'' });
const remote = (path='s3://bucket/dest/', s3Id='connection') => ({ backend:'s3', path, s3Id, profileId:'profile', bucket:'bucket' });
const definition = () => ({ id:'job', revision:'1', name:'Publish', source:{kind:'local',path:'/original',device:'1',inode:'2'}, destination:{profile_id:'profile',generation:'native-generation',prefix:'dest/',configuration:{bucket:'bucket',region:'us-east-1',endpoint:'https://provider.test'}}, exclude_patterns:['*.tmp'],compare_mode:'sha256',operation:'copy_update_only' });
const report = () => ({revision:'store',configuration_revision:'config',execution_available:false,jobs:[{definition:definition(),status:'definition_valid',reason:'Valid settings'}],profiles:[{id:'profile',name:'Saved destination',configuration:definition().destination.configuration,unavailable:null}]});
let serial = 0;
async function wizard(methods={}, props={}) {
  const key=`syncGuide${serial++}`; const calls=[]; const mounts=[];
  const fixture={calls,mounts,props:{source:local('/original'),destination:remote(),locations:[local('/original'),remote()],onSync:(...args)=>calls.push(['transfer',...args]),onClose(){},...props},methods:{load:async()=>report(),save:async request=>({...report(),jobs:[{definition:{...definition(),name:request.name,source:{kind:'local',path:request.source.path},exclude_patterns:request.exclude_patterns},status:'definition_valid'}]}),...methods}};
  globalThis[key]=fixture;
  const dependencies={
    svelte:data(`export const untrack=callback=>callback();export const onMount=callback=>globalThis.${key}.mounts.push(callback);`),
    '#lib/utils/comparison-options.js':new URL('../src/lib/utils/comparison-options.ts',import.meta.url).href,
    '#lib/utils/sync-workflow.js':new URL('../src/lib/utils/sync-workflow.ts',import.meta.url).href,
    '#lib/state/app.svelte.js':data('export const appState={syncExcludePatterns:"*.tmp"};'),
    '#lib/services/saved-jobs.js':data(`const call=(name,...args)=>{const f=globalThis.${key};f.calls.push([name,...args]);return f.methods[name](...args);};export const listSavedJobs=()=>call('load');export const saveJobDefinition=(...args)=>call('save',...args);`),
  };
  let script=file('lib/components/SyncWizard.svelte').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replaceAll('$props()',`globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm,'');
  script+=`\nexport const harness={load,chooseSaved,changeSource,changeDestination,nextSource,nextDestination,back,transfer,save(nameValue,excludeValue){name=nameValue;exclusions=excludeValue;saveSetup=true;},get state(){return{step,source,destination,saved,busy,failure,report,saveSetup,reviewMode};}};`;
  const code=compileModule(stripTypeScriptTypes(script),{generate:'client',filename:'Guide.js'}).js.code;
  const {harness:h}=await import(data(imports(code,dependencies))); let destroy;
  return{h,...fixture,mount(){destroy=mounts[0]();},close(){destroy?.();},cleanup(){delete globalThis[key];}};
}

test('locations reject identical/overlapping roots and bucket changes without rewriting literal prefixes',()=>{
  assert.match(syncLocationError(local('/a'),local('/a')),/non-overlapping/);
  assert.match(syncLocationError(local('/a'),local('/a/nested')),/non-overlapping/);
  assert.match(syncLocationError(local('/'),local('/b')),/non-overlapping/);
  assert.equal(syncLocationError(local('/a'),local('/ab')),'');
  assert.ok(locationError(local('/a/../b'))); assert.ok(locationError(local('relative')));
  assert.ok(locationError({...remote(),path:'s3://other/dest/'}));
  assert.ok(locationError(remote('s3://bucket/folder')));
  assert.match(syncLocationError(remote('s3://bucket/a/'),remote('s3://bucket/a/nested/')),/non-overlapping/);
  assert.equal(syncLocationError(remote(),remote('s3://bucket/dest/','other-provider')),'');
  assert.deepEqual(s3Folder(remote('s3://bucket/literal%20name//')),{bucket:'bucket',prefix:'literal%20name//'});
});

test('three-step setup does not scan, connect, save or transfer on opening, navigation or optional saving being absent',async()=>{
  const f=await wizard();try{
    assert.equal(f.h.state.step,1);assert.equal(f.calls.length,0);await f.h.load();assert.deepEqual(f.calls.map(c=>c[0]),['load']);
    f.h.nextSource();assert.equal(f.h.state.step,2);await f.h.nextDestination();assert.equal(f.h.state.step,3);assert.equal(f.calls.length,1);
    f.props.source.path='/pane-changed';f.props.destination.path='s3://bucket/different/';assert.equal(f.h.state.source.path,'/original');assert.equal(f.h.state.destination.path,'s3://bucket/dest/');
    f.h.transfer([{relative_path:'file',status:'new'}]);assert.deepEqual(f.calls[1].slice(2),[local('/original'),remote()]);
    f.h.back();f.h.transfer([]);assert.equal(f.calls.length,2);f.h.changeDestination(local('/original/nested'));await f.h.nextDestination();assert.equal(f.h.state.step,2);
  }finally{f.cleanup();}
});

test('saving is explicit revision-bound configuration only, preserves exclusions and switches to owned native job workflow',async()=>{
  const f=await wizard();try{await f.h.load();f.h.nextSource();f.h.save('Publish','nested/**, *.tmp');await f.h.nextDestination();assert.deepEqual(f.calls.map(c=>c[0]),['load','save']);const request=f.calls[1][1];assert.deepEqual(request,{expected_revision:'store',expected_configuration_revision:'config',id:null,name:'Publish',source:{kind:'local',path:'/original'},destination_profile_id:'profile',destination_prefix:'dest/',exclude_patterns:['nested/**','*.tmp'],acknowledged:true});assert.equal(f.h.state.step,3);assert.equal(f.h.state.saved.id,'job');assert.equal(f.h.state.saveSetup,false);f.h.transfer([]);assert.equal(f.calls.length,2);}finally{f.cleanup();}
});

test('selecting a saved job starts no native connection/comparison and stale definitions cannot enter execution workflow',async()=>{
  const disk=report();disk.jobs.push({definition:{...definition(),id:'stale'},status:'needs_rebind',reason:'Profile changed'});const f=await wizard({load:async()=>disk});
  try{await f.h.load();f.h.chooseSaved('stale');assert.equal(f.h.state.step,1);assert.match(String(f.h.state.failure),/Profile changed/);f.h.chooseSaved('job');assert.equal(f.h.state.step,3);assert.deepEqual(f.calls.map(c=>c[0]),['load']);assert.notEqual(f.h.state.saved,disk.jobs[0].definition);}finally{f.cleanup();}
});

test('failed/late saves cannot advance a closed wizard, double-save, overwrite confirmed reports or authorize transfers',async()=>{
  const pending=deferred();const f=await wizard({save:async()=>pending.promise});
  try{f.mount();await Promise.resolve();await Promise.resolve();f.h.nextSource();f.h.save('Publish','');const saving=f.h.nextDestination();await f.h.nextDestination();assert.equal(f.calls.filter(c=>c[0]==='save').length,1);f.close();pending.resolve(report());await saving;assert.equal(f.h.state.step,2);f.h.transfer([]);assert.equal(f.calls.filter(c=>c[0]==='transfer').length,0);}finally{f.cleanup();}
  const bad=await wizard({save:async()=>{throw new Error('Configuration revision changed');}});try{await bad.h.load();bad.h.nextSource();bad.h.save('Publish','');await bad.h.nextDestination();assert.equal(bad.h.state.step,2);assert.equal(bad.h.state.report.revision,'store');assert.equal(bad.h.state.saveSetup,false);assert.match(String(bad.h.state.failure),/revision changed/);}finally{bad.cleanup();}
});

async function comparisonFixture(){
  const key=`comparisonGuide${serial++}`;const f={calls:[],scans:[],app:{modal:null,showSync(...args){f.calls.push(args);this.modal='sync';}}};globalThis[key]=f;
  const dependencies={
    '#lib/services/tauri.js':data(`export const syncDiff=async(...args)=>globalThis.${key}.scans.push(args);export const cancelSync=async()=>{};`),
    '#lib/state/app.svelte.js':data(`export const appState=globalThis.${key}.app;`),
    '#lib/state/status.svelte.js':data('export const statusState={setMessage(){}};'),
    '#lib/utils/comparison-options.js':new URL('../src/lib/utils/comparison-options.ts',import.meta.url).href,
    '#lib/utils/file-diff.js':new URL('../src/lib/utils/file-diff.ts',import.meta.url).href,
    '#lib/utils/error-report.js':new URL('../src/lib/utils/error-report.ts',import.meta.url).href,
  };
  const code=compileModule(stripTypeScriptTypes(file('lib/state/comparison.svelte.ts')),{generate:'client',filename:'Comparison.js'}).js.code;
  const {comparisonState:state}=await import(data(imports(code,dependencies)));return{...f,state,cleanup(){state.stopComparison();delete globalThis[key];}};
}

test('all-backend Compare retains exact SFTP connection IDs through retry/content verification without granting Sync or text-diff authority', async () => {
  for (const remote of ['sftp', 'archive']) {
    const f = await comparisonFixture();
    try {
      const path = remote === 'sftp' ? 'sftp://original:22/folder/' : 'archive:///fixture.zip#folder';
      await f.state.startComparison(path, remote, remote === 'sftp' ? 'original-session' : '', '/local', 'local', '', 'size_mtime', ['*.tmp']);
      assert.equal(f.scans.length, 1); assert.equal(f.scans[0][1], remote); assert.equal(f.scans[0][2], path);
      assert.equal(f.scans[0][3], remote === 'sftp' ? 'original-session' : '');
      const event = f.scans[0][9];
      event({ type: 'Entry', relative_path: 'nested/file', status: 'modified' });
      event({ type: 'Done', new_count: 0, modified: 1, deleted: 0 });
      const child = remote === 'archive' ? 'archive:///fixture.zip#folder/nested' : 'sftp://original:22/folder/nested/';
      const entry = { name: 'file', is_dir: false };
      assert.equal(f.state.statusForEntry('left', child, entry), 'modified');
      assert.equal(f.state.syncSetup('left'), null); assert.equal(f.state.syncSetup('right'), null);
      assert.equal(f.state.diffForEntry('left', child, entry), null);
      f.state.openSync('left'); assert.equal(f.calls.length, 0);
      await f.state.verifyContents(); assert.equal(f.scans[1][8], 'sha256');
      assert.equal(f.scans[1][2], path); assert.equal(f.scans[1][3], f.scans[0][3]);
      await f.state.retryComparison(); assert.deepEqual(f.scans[2].slice(1, 9), f.scans[1].slice(1, 9));
      event({ type: 'Done', new_count: 9, modified: 9, deleted: 9 });
      assert.equal(f.state.scanning, true); assert.equal(f.state.outcome, undefined);
    } finally { f.cleanup(); }
  }
});

test('archive root and nested comparison colours follow the archive URI boundary, not filesystem slash rules', async () => {
  const f = await comparisonFixture();
  try {
    await f.state.startComparison('archive:///fixture.zip#', 'archive', '', '/local', 'local', '');
    f.scans[0][9]({ type: 'Entry', relative_path: 'folder/file', status: 'new' });
    f.scans[0][9]({ type: 'Done', new_count: 1, modified: 0, deleted: 0 });
    assert.equal(f.state.statusForEntry('left', 'archive:///fixture.zip#folder', { name: 'file', is_dir: false }), 'new');
    assert.equal(f.state.statusForEntry('left', 'archive:///other.zip#folder', { name: 'file', is_dir: false }), undefined);
  } finally { f.cleanup(); }
});

test('Compare only offers setup after complete results and hands off exact original roots/IDs/options in either direction, never rows or approvals',async()=>{
  for(const side of ['left','right']){const f=await comparisonFixture();try{
    await f.state.startComparison('/original','local','','s3://bucket/dest/','s3','old-connection','sha256',['secret/**']);assert.equal(f.state.syncSetup(side),null);f.state.openSync(side);assert.equal(f.calls.length,0);
    const events=f.scans[0][9];events({type:'Done',new_count:1,modified:0,deleted:1});const setup=f.state.syncSetup(side);assert.deepEqual(setup.options,{mode:'sha256',exclusions:['secret/**'],fromComparison:true});setup.options.exclusions.push('caller-mutation');assert.deepEqual(f.state.syncSetup(side).options.exclusions,['secret/**']);f.state.openSync(side);assert.equal(f.calls.length,1);const [source,destination,options]=f.calls[0];assert.deepEqual(source,side==='left'?local('/original'):{backend:'s3',path:'s3://bucket/dest/',s3Id:'old-connection'});assert.equal(destination.path,side==='left'?'s3://bucket/dest/':'/original');assert.deepEqual(Object.keys(options).sort(),['exclusions','fromComparison','mode']);assert.equal(f.state.active,false);assert.equal(f.state.syncSetup(side),null);
  }finally{f.cleanup();}}
  for(const outcome of ['failed','limited','cancelled']){const f=await comparisonFixture();try{await f.state.startComparison('/a','local','','/b','local','');f.scans[0][9]({type:'ScanStopped',outcome,message:'Incomplete',progress:{}});f.state.openSync('left');assert.equal(f.calls.length,0);}finally{f.cleanup();}}
});

test('guide renders source/destination/review steps without the old flat form or any automatic read/transfer approval',async()=>{
  const dependencies={
    svelte:import.meta.resolve('svelte'),
    '#lib/utils/comparison-options.js':new URL('../src/lib/utils/comparison-options.ts',import.meta.url).href,
    '#lib/utils/sync-workflow.js':new URL('../src/lib/utils/sync-workflow.ts',import.meta.url).href,
    '#lib/state/app.svelte.js':data('export const appState={syncExcludePatterns:""};'),
    '#lib/services/saved-jobs.js':data('export const listSavedJobs=async()=>{throw new Error("No SSR IO");};export const saveJobDefinition=async()=>{throw new Error("No auto save");};'),
  };
  for(const component of ['SyncLocationPicker','SyncDialog','JobComparisonSession','SavedSyncJobs','JobHistory','ErrorNotice'])dependencies[`./${component}.svelte`]=data('export default ()=>{};');
  const code=compile(file('lib/components/SyncWizard.svelte'),{generate:'server',filename:'Guide.svelte'}).js.code;const {default:Guide}=await import(data(imports(code,dependencies)));const html=render(Guide,{props:{source:local('/source'),destination:local('/dest'),onSync(){},onClose(){}}}).body;assert.ok(html.includes('1 · Source'));assert.ok(html.includes('2 · Destination'));assert.ok(html.includes('3 · Review'));assert.ok(html.includes('Where are the original files?'));assert.ok(!html.includes('Pinned definitions (foundation)'));assert.ok(!html.includes('Execute approved writes once'));assert.ok(!/<details[^>]*\bopen\b/.test(html));
});

test('actual page dispatches the reviewed edited roots, not stale pane/modal values reset on close',()=>{
  const script=file('routes/+page.svelte').match(/function handleSyncExecute[\s\S]*?(?=\n {2}function hideQuake)/)[0];const dispatched=[];const app={syncSourcePath:'/old-source',syncDestPath:'/old-destination',closeModal(){this.syncSourcePath='';this.syncDestPath='';}};
  const handle=new Function('appState','window','CustomEvent',stripTypeScriptTypes(script)+';return handleSyncExecute;')(app,{dispatchEvent:event=>dispatched.push(event)},class{constructor(type,options){this.type=type;this.detail=options.detail;}});
  handle([{relative_path:'file',status:'new'}],local('/chosen-source'),remote('s3://bucket/chosen/'));
  assert.equal(dispatched[0].type,'sync-execute');assert.equal(dispatched[0].detail.sourcePath,'/chosen-source');assert.equal(dispatched[0].detail.destPath,'s3://bucket/chosen/');assert.equal(dispatched[0].detail.destS3Id,'connection');
});

test('actual Compare page callback uses SFTP IDs only for SFTP and never stale remote IDs for local/archive panes', () => {
  const body = file('routes/+page.svelte').match(/onCompare=\{\(mode, excludes\) => \{([\s\S]*?)\n {6}\}\}/)[1];
  const compare = new Function('panels', 'appState', 'comparisonState', 'mode', 'excludes', body);
  for (const backend of ['local', 's3', 'sftp', 'archive']) {
    const calls = [];
    const panel = { path: '/original', backend, s3Connection: { connectionId: 's3-only' }, sftpConnection: { connectionId: 'sftp-only' } };
    compare({ left: panel, right: panel }, { closeModal() {} }, { startComparison(...args) { calls.push(args); } }, 'sha256', ['*.tmp']);
    const expected = backend === 'sftp' ? 'sftp-only' : backend === 's3' ? 's3-only' : '';
    assert.equal(calls[0][2], expected); assert.equal(calls[0][5], expected); assert.deepEqual(calls[0].slice(6), ['sha256', ['*.tmp']]);
  }
});
