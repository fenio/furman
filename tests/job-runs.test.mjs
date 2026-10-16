import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';

const data=code=>`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const source=readFileSync(new URL('../src/lib/components/JobComparisonSession.svelte',import.meta.url),'utf8');
const deferred=()=>{let resolve;let reject;const promise=new Promise((ok,fail)=>{resolve=ok;reject=fail;});return{promise,resolve,reject};};
const definition={id:'job',revision:'3',name:'Saved job',source:{kind:'local',path:'/saved/source',device:'1',inode:'2'},destination:{profile_id:'saved-target',generation:'generation',configuration:{bucket:'bucket',region:'us-east-1',endpoint:'https://us-ord-1.linodeobjects.com/',provider:'linode',proxy_endpoint:null,proxy_auth:false,sse_customer_key_enabled:true,acceleration:false},prefix:'target//'},exclude_patterns:[],compare_mode:'sha256',operation:'copy_update_only'};
const progress={scanned:2,matched:0,requests:2,bytes_read:3,s3_bytes_read:3,elapsed_ms:100};
const bound=()=>({run_id:crypto.randomUUID(),job_id:'job',phase:'bound',expires_seconds:900,outcome:null,message:null,progress,summary:null,manifest_id:null,source_instance:null,destination_instance:crypto.randomUUID(),source_namespace:null,destination_namespace:'https://us-ord-1.linodeobjects.com/',limits:{max_objects:100000,max_requests:1000,max_seconds:300,max_results:1000},execution_available:false});
const complete=status=>({...status,phase:'complete',outcome:'complete',summary:{new:1,modified:0,destination_only:0,same:2},manifest_id:crypto.randomUUID()});
const eligible=(run,manifest)=>({...complete(bound()),run_id:run,manifest_id:manifest,phase:'prepared',preview:{id:'native-preview',can_review_execution:true,execution_available:false}});
function imports(code,dependencies){return code.replace(/from (['"])([^'"]+)\1/g,(_,quote,name)=>{const resolved=dependencies[name]??(name==='#lib/state/navigation-cache.svelte.js'?data('export const beginS3Mutation=()=>()=>{};'):name.startsWith('svelte')?import.meta.resolve(name):null);assert.ok(resolved,`Unmapped job-run import ${name}`);return `from ${JSON.stringify(resolved)}`;});}
let sequence=0;
async function fixture(methods={}){
  const key=`jobRunFixture${sequence++}`;const calls=[];const destroys=[];const initial=bound();
  const context={props:{definition:structuredClone(definition)},destroys,calls,methods:{bind:async()=>initial,scan:async()=>complete(initial),page:async()=>[{id:'entry',relative_path:'nested/file',status:'new',source_size:3,destination_size:null}],release:async()=>{},status:async()=>initial,...methods}};
  globalThis[key]=context;
  const rpc=data(`const run=(name,...args)=>{const fixture=globalThis.${key};fixture.calls.push([name,...args]);return fixture.methods[name](...args);};export const bindJobSession=(...args)=>run('bind',...args);export const scanJobSession=(...args)=>run('scan',...args);export const releaseJobSession=(...args)=>run('release',...args);export const jobSessionStatus=(...args)=>run('status',...args);export const jobManifestPage=(...args)=>run('page',...args);export const prepareJobPreview=(...args)=>run('prepare',...args);export const reviewJobExecution=(...args)=>run('review',...args);export const executeJobPreview=(...args)=>run('execute',...args);`);
  let script=source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replaceAll('$props()',`globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm,'');
  script+=`\nexport const harness={bind,compare,more,refresh,release,prepare,reviewExecution,execute,toggleEntry,approve(){approved=true;},approveSnapshots(){snapshotApproved=true;},approveExecution(){executeApproved=true;exclusionsApproved=true;},approveReplacements(){replacementsApproved=true;},setBudget(value){snapshotMiB=value;},get state(){return{approved,session,progress,rows,busy,failure,status,opId,selectedIds,snapshotApproved,executeApproved,exclusionsApproved,replacementsApproved};}};`;
  const code=compileModule(stripTypeScriptTypes(script),{generate:'client',filename:'JobComparisonSession.svelte.js'}).js.code;
  const {harness}=await import(data(imports(code,{'svelte':data(`export const onDestroy=callback=>globalThis.${key}.destroys.push(callback);`),'#lib/services/job-runs.js':rpc,'#lib/services/log.js':data('export const error=()=>{};')})));
  return{h:harness,calls,context,close(){for(const destroy of destroys)destroy();},cleanup(){delete globalThis[key];}};
}

test('actual read-only controller requires separate binding/compare actions and pages only native complete handles',async()=>{
  const f=await fixture();try{
    assert.equal(f.calls.length,0);await f.h.bind();assert.equal(f.calls.length,0);await f.h.compare();assert.equal(f.calls.length,0);
    f.h.approve();await f.h.bind();assert.deepEqual(f.calls.map(call=>call[0]),['bind']);assert.equal(f.calls[0][2],'job');assert.equal(f.calls[0][3],'3');assert.equal(f.calls[0][4],true);assert.equal(f.h.state.approved,false);
    await f.h.compare();assert.deepEqual(f.calls.map(call=>call[0]),['bind','scan','page']);assert.equal(f.h.state.session.phase,'complete');assert.equal(f.h.state.rows[0].relative_path,'nested/file');assert.equal(f.calls[2][1],f.h.state.session.run_id);assert.equal(f.calls[2][2],f.h.state.session.manifest_id);assert.equal(f.calls[2][3],0);
    assert.ok(!f.calls.some(call=>/execute|delete|upload|sync/i.test(call[0])));
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('cancelled scans discard late progress/completion and cannot fetch a manifest',async()=>{
  const pending=deferred();let onProgress;const f=await fixture({scan:async(_id,notify)=>{onProgress=notify;return pending.promise;}});
  try{
    f.h.approve();await f.h.bind();const session=f.h.state.session;const scanning=f.h.compare();onProgress(progress);assert.equal(f.h.state.progress.bytes_read,3);
    await f.h.release();onProgress({...progress,bytes_read:999});pending.resolve(complete(session));await scanning;
    assert.equal(f.h.state.session,null);assert.equal(f.h.state.progress,null);assert.deepEqual(f.h.state.rows,[]);assert.ok(!f.calls.some(call=>call[0]==='page'));
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('closing while native binding is queued cancels the operation and releases a late bound run',async()=>{
  const pending=deferred();const f=await fixture({bind:async()=>pending.promise});
  try{
    f.h.approve();const binding=f.h.bind();const operation=f.h.state.opId;f.close();const late=bound();pending.resolve(late);await binding;await Promise.resolve();
    assert.ok(f.calls.some(call=>call[0]==='release'&&call[1]===operation));assert.ok(f.calls.some(call=>call[0]==='release'&&call[1]===late.run_id));assert.equal(f.h.state.session,null);assert.ok(!f.calls.some(call=>call[0]==='scan'));
  }finally{f.cleanup();}
});

test('limited/failed native scans retain progress but never read rows or enable execution',async()=>{
  const f=await fixture({scan:async()=>({...bound(),phase:'stopped',outcome:'limited',message:'Budget exhausted',progress,manifest_id:null})});
  try{f.h.approve();await f.h.bind();await f.h.compare();assert.equal(f.h.state.session.phase,'stopped');assert.equal(f.h.state.progress.bytes_read,3);assert.ok(!f.calls.some(call=>call[0]==='page'));assert.equal(f.h.state.session.execution_available,false);}finally{f.close();await Promise.resolve();f.cleanup();}
});

test('expired/stale manifest responses clear rows rather than retaining usable comparison state',async()=>{
  const f=await fixture({page:async()=>{throw new Error('Manifest expired or configuration changed');}});
  try{f.h.approve();await f.h.bind();await f.h.compare();assert.equal(f.h.state.session,null);assert.deepEqual(f.h.state.rows,[]);assert.match(String(f.h.state.failure),/Manifest expired/);assert.ok(f.calls.some(call=>call[0]==='release'));}finally{f.close();await Promise.resolve();f.cleanup();}
});

test('read-only service sends no credentials or caller-selected rows and cannot bind without approval',async()=>{
  const calls=[];globalThis.__jobRunRpc=(...args)=>{calls.push(args);return Promise.resolve(bound());};
  try{
    const script=stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-runs.ts',import.meta.url),'utf8'));
    const {bindJobSession,scanJobSession,jobManifestPage,releaseJobSession,jobSessionStatus}=await import(data(imports(script,{'@tauri-apps/api/core':data('export const invoke=(...args)=>globalThis.__jobRunRpc(...args);export class Channel{}'),'#lib/services/job-journal.js':data('export const noteJobJournalFailure=()=>{};'),'#lib/state/job-execution.svelte.js':data('export const jobExecutionState={status:null,error:null};')})));
    await assert.rejects(bindJobSession('op','job','rev',false));assert.equal(calls.length,0);await bindJobSession('op','job','rev',true);
    let progressSeen;await scanJobSession('native-run',event=>{progressSeen=event;});calls[1][1].channel.onmessage(progress);assert.equal(progressSeen.bytes_read,3);
    await jobManifestPage('native-run','native-manifest',0);await jobSessionStatus('native-run');await releaseJobSession('native-run');
    assert.deepEqual(calls.map(call=>call[0]),['saved_job_bind','saved_job_scan','saved_job_manifest_page','saved_job_status','saved_job_release']);
    assert.deepEqual(calls[0][1],{opId:'op',jobId:'job',revision:'rev',approved:true});assert.deepEqual(calls[2][1],{runId:'native-run',manifestId:'native-manifest',offset:0});
  }finally{delete globalThis.__jobRunRpc;}
});

test('actual session UI renders separate read-only approval and keeps comparison disabled before binding',async()=>{
  const dependencies={'#lib/services/job-runs.js':data('export const bindJobSession=async()=>{throw new Error("No SSR binding");};export const scanJobSession=async()=>{};export const releaseJobSession=async()=>{};export const jobSessionStatus=async()=>{};export const jobManifestPage=async()=>{};export const prepareJobPreview=async()=>{};export const reviewJobExecution=async()=>{};export const executeJobPreview=async()=>{};'),'#lib/services/log.js':data('export const error=()=>{};'),'#lib/utils/error-report.js':new URL('../src/lib/utils/error-report.ts',import.meta.url).href,'#lib/utils/scan.js':new URL('../src/lib/utils/scan.ts',import.meta.url).href,'#lib/utils/format.js':new URL('../src/lib/utils/format.ts',import.meta.url).href};
  const component=file=>data(imports(compile(readFileSync(new URL(`../src/lib/components/${file}`,import.meta.url),'utf8'),{generate:'server',filename:file}).js.code,dependencies));
  dependencies['./ErrorNotice.svelte']=component('ErrorNotice.svelte');dependencies['./ScanStatus.svelte']=component('ScanStatus.svelte');
  const {default:Component}=await import(data(imports(compile(source,{generate:'server',filename:'JobComparisonSession.svelte'}).js.code,dependencies)));
  const html=render(Component,{props:{definition}}).body;assert.ok(html.includes('not unsaved editor fields'));assert.ok(html.includes('HeadBucket'));assert.match(html,/disabled[^>]*>Compare saved job/);assert.ok(html.includes('S3 → S3 writes up to 5 MiB/file'));assert.ok(html.includes('No unguarded overwrite, multipart jobs'));assert.ok(!html.includes('Upload now'));
});

test('native S3 execution review renders preservation and risk disclosures without enabling unacknowledged writes',async()=>{
  const initial={...bound(),phase:'awaiting_execution_approval',execution_available:true,execution_review:{approval_id:'native-only',source:'source-native',destination:'destination-native',exclusions:['*.skip'],destination_sse_c:true,s3_source:true,preserved_tag_count:2,source_aes256_files:0,source_sse_c_files:1,preview:{revision:'3',policy:'create_only',files:1,bytes:3,skipped:0,expires_seconds:123,entries:[{entry_id:'owned-entry',relative_path:'<source>.txt',destination_key:'dest/<source>.txt',size:3}]}}};
  const dependencies={'#lib/services/job-runs.js':data('export const bindJobSession=async()=>{};export const scanJobSession=async()=>{};export const releaseJobSession=async()=>{};export const jobSessionStatus=async()=>{};export const jobManifestPage=async()=>{};export const prepareJobPreview=async()=>{};export const reviewJobExecution=async()=>{};export const executeJobPreview=async()=>{throw new Error("SSR cannot execute");};'),'#lib/services/log.js':data('export const error=()=>{};'),'#lib/utils/error-report.js':new URL('../src/lib/utils/error-report.ts',import.meta.url).href,'#lib/utils/scan.js':new URL('../src/lib/utils/scan.ts',import.meta.url).href,'#lib/utils/format.js':new URL('../src/lib/utils/format.ts',import.meta.url).href};
  const component=file=>data(imports(compile(readFileSync(new URL(`../src/lib/components/${file}`,import.meta.url),'utf8'),{generate:'server',filename:file}).js.code,dependencies));dependencies['./ErrorNotice.svelte']=component('ErrorNotice.svelte');dependencies['./ScanStatus.svelte']=component('ScanStatus.svelte');
  const rendering=source.replace('let session = $state.raw<JobRunStatus | null>(null);',`let session = $state.raw<JobRunStatus | null>(${JSON.stringify(initial)});`);const{default:Component}=await import(data(imports(compile(rendering,{generate:'server',filename:'S3ExecutionReview.svelte'}).js.code,dependencies)));const html=render(Component,{props:{definition}}).body;
  assert.ok(html.includes('source-native'));assert.ok(html.includes('Content-Disposition'));assert.ok(html.includes('Source ACLs must be recognized owner-only'));assert.ok(html.includes('destination ownership belongs to its new owner'));assert.ok(html.includes('not an atomic snapshot'));assert.ok(html.includes('No KMS source migration'));assert.ok(html.includes('&lt;source>'));assert.ok(!html.includes('<source>'));assert.match(html,/disabled[^>]*>Execute approved writes once/);
});

test('checked preparation requires explicit loaded native entry selection and separate snapshot approval',async()=>{
  const f=await fixture({prepare:async(run,manifest)=>({...complete(bound()),run_id:run,manifest_id:manifest,phase:'prepared',preview:{id:'native-preview',execution_available:false}})});
  try{
    await f.h.prepare();assert.equal(f.calls.length,0);
    f.h.approve();await f.h.bind();await f.h.compare();assert.ok(!f.calls.some(call=>call[0]==='prepare'));
    f.h.toggleEntry('synthetic');assert.equal(f.h.state.selectedIds.size,0);f.h.toggleEntry('entry');await f.h.prepare();assert.ok(!f.calls.some(call=>call[0]==='prepare'));
    f.h.approveSnapshots();await f.h.prepare();const call=f.calls.find(call=>call[0]==='prepare');assert.deepEqual(call[3],['entry']);assert.equal(call[4],'create_only');assert.equal(call[5],1024*1024*1024);assert.equal(call[6],true);
    assert.equal(f.h.state.snapshotApproved,false);assert.equal(f.h.state.session.phase,'prepared');assert.equal(f.h.state.session.execution_available,false);
    await f.h.prepare();assert.equal(f.calls.filter(call=>call[0]==='prepare').length,1);assert.ok(!f.calls.some(call=>/execute|upload|delete/.test(call[0])));
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('selection changes reset snapshot approval and invalid disk budgets never reach native preparation',async()=>{
  const f=await fixture();try{
    f.h.approve();await f.h.bind();await f.h.compare();f.h.toggleEntry('entry');f.h.approveSnapshots();f.h.toggleEntry('entry');assert.equal(f.h.state.snapshotApproved,false);f.h.toggleEntry('entry');f.h.approveSnapshots();f.h.setBudget(1.5);await f.h.prepare();assert.match(String(f.h.state.failure),/whole snapshot disk budget/);assert.ok(!f.calls.some(call=>call[0]==='prepare'));
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('cancelled snapshot preparation ignores late progress and cannot restore a prepared handle',async()=>{
  const pending=deferred();let notify;const f=await fixture({prepare:async(_run,_manifest,_ids,_policy,_bytes,_approved,onProgress)=>{notify=onProgress;return pending.promise;}});
  try{
    f.h.approve();await f.h.bind();await f.h.compare();f.h.toggleEntry('entry');f.h.approveSnapshots();const preparing=f.h.prepare();assert.equal(f.h.state.busy,'preparing');
    await f.h.release();notify(progress);pending.resolve({...complete(bound()),phase:'prepared',preview:{id:'late-preview'}});await preparing;
    assert.equal(f.h.state.session,null);assert.equal(f.h.state.progress,null);assert.equal(f.h.state.selectedIds.size,0);assert.equal(f.h.state.snapshotApproved,false);
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('snapshot service sends only native ownership handles, selected IDs and reviewed limits/policy',async()=>{
  const calls=[];globalThis.__jobSnapshotRpc=(...args)=>{calls.push(args);return Promise.resolve(bound());};
  try{
    const script=stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-runs.ts',import.meta.url),'utf8'));
    const {prepareJobPreview}=await import(data(imports(script,{'@tauri-apps/api/core':data('export const invoke=(...args)=>globalThis.__jobSnapshotRpc(...args);export class Channel{}'),'#lib/services/job-journal.js':data('export const noteJobJournalFailure=()=>{};'),'#lib/state/job-execution.svelte.js':data('export const jobExecutionState={status:null,error:null};')})));
    await assert.rejects(prepareJobPreview('run','manifest',['entry'],'create_only',100,false,()=>{}));assert.equal(calls.length,0);
    await prepareJobPreview('run','manifest',['entry'],'replace_unchanged',100,true,()=>{});
    assert.equal(calls[0][0],'saved_job_prepare');assert.deepEqual(calls[0][1].request,{run_id:'run',manifest_id:'manifest',entry_ids:['entry'],policy:'replace_unchanged',max_snapshot_bytes:100,approved:true});
    const encoded=JSON.stringify(calls);assert.ok(!encoded.includes('expected_sha256'));assert.ok(!encoded.includes('source_path'));assert.ok(!encoded.includes('credential'));assert.ok(!encoded.includes('overwrite'));
  }finally{delete globalThis.__jobSnapshotRpc;}
});

test('execution requires distinct native review and two explicit acknowledgements; double clicks cannot replay',async()=>{
  const pending=deferred();const f=await fixture({prepare:async(run,manifest)=>eligible(run,manifest),review:async run=>({...bound(),run_id:run,phase:'awaiting_execution_approval',execution_available:true,execution_review:{approval_id:'native-single-use'}}),execute:async()=>pending.promise});
  try{
    f.h.approve();await f.h.bind();await f.h.compare();f.h.toggleEntry('entry');f.h.approveSnapshots();await f.h.prepare();
    assert.equal(f.calls.filter(c=>c[0]==='execute').length,0);await f.h.execute();assert.equal(f.calls.filter(c=>c[0]==='execute').length,0);
    await f.h.reviewExecution();assert.deepEqual(f.calls.at(-1),['review',f.h.state.session.run_id,'native-preview']);await f.h.execute();assert.equal(f.calls.filter(c=>c[0]==='execute').length,0);
    f.h.approveExecution();const executing=f.h.execute();await f.h.execute();assert.equal(f.calls.filter(c=>c[0]==='execute').length,1);assert.deepEqual(f.calls.at(-1).slice(2),['native-single-use',true,true,false]);assert.equal(f.h.state.executeApproved,false);assert.equal(f.h.state.session.execution_review,null);
    pending.resolve({...bound(),phase:'stopped',execution:{status:'succeeded'}});await executing;assert.equal(f.h.state.session.phase,'stopped');
  }finally{f.close();await Promise.resolve();f.cleanup();}
});

test('existing-object replacements require an additional acknowledgement that cannot come from preparation or create approval',async()=>{
  const f=await fixture({prepare:async(run,manifest)=>eligible(run,manifest),review:async run=>({...bound(),run_id:run,phase:'awaiting_execution_approval',execution_available:true,execution_review:{approval_id:'replacement-native',replacement_files:1}}),execute:async()=>({...bound(),phase:'stopped',execution:{status:'succeeded'}})});
  try{f.h.approve();await f.h.bind();await f.h.compare();f.h.toggleEntry('entry');f.h.approveSnapshots();await f.h.prepare();await f.h.reviewExecution();f.h.approveExecution();await f.h.execute();assert.equal(f.calls.filter(c=>c[0]==='execute').length,0);f.h.approveReplacements();await f.h.execute();assert.equal(f.calls.filter(c=>c[0]==='execute').length,1);assert.deepEqual(f.calls.find(c=>c[0]==='execute').slice(2),['replacement-native',true,true,true]);assert.equal(f.h.state.replacementsApproved,false);}finally{f.close();await Promise.resolve();f.cleanup();}
});

test('replacement IPC explicitly carries acknowledgement, never caller-selected validators or attributes',async()=>{
  const calls=[];globalThis.__replacementRPC=async(...args)=>{calls.push(args);return{phase:'stopped'};};
  try{const script=stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-runs.ts',import.meta.url),'utf8'));const service=await import(data(imports(script,{'@tauri-apps/api/core':data('export const invoke=(...args)=>globalThis.__replacementRPC(...args);export class Channel{}'),'#lib/services/job-journal.js':data('export const noteJobJournalFailure=()=>{};'),'#lib/state/job-execution.svelte.js':data('export const jobExecutionState={status:null,error:null};')})));await service.executeJobPreview('native-run','native-approval',true,true,true);assert.deepEqual(calls,[['saved_job_execute',{request:{run_id:'native-run',approval_id:'native-approval',approved:true,acknowledge_exclusions:true,acknowledge_replacements:true}}]]);}finally{delete globalThis.__replacementRPC;}
});

test('closing during native execution cancels by run ID and ignores late results rather than replaying',async()=>{
  const pending=deferred();const f=await fixture({prepare:async(run,manifest)=>eligible(run,manifest),review:async run=>({...bound(),run_id:run,phase:'awaiting_execution_approval',execution_available:true,execution_review:{approval_id:'native-single-use'}}),execute:async()=>pending.promise});
  try{f.h.approve();await f.h.bind();await f.h.compare();f.h.toggleEntry('entry');f.h.approveSnapshots();await f.h.prepare();await f.h.reviewExecution();f.h.approveExecution();const id=f.h.state.session.run_id;const executing=f.h.execute();f.close();pending.resolve({...bound(),phase:'stopped',execution:{status:'uncertain'}});await executing;assert.ok(f.calls.some(c=>c[0]==='release'&&c[1]===id));assert.equal(f.h.state.session.phase,'executing');assert.equal(f.calls.filter(c=>c[0]==='execute').length,1);}finally{f.cleanup();}
});

test('execution service sends only native IDs and approvals, monitors locally and retains outcomes independently of dialogs',async()=>{
  const calls=[];const shared={status:null,error:null};const notices=[];const cache=[];globalThis.__executionCache=value=>cache.push(value);globalThis.__executionShared=shared;globalThis.__executionNotice=value=>notices.push(value);
  globalThis.__executionRpc=async(command,args)=>{calls.push([command,args]);if(command==='saved_job_execute')return{...bound(),run_id:'native-run',phase:'executing',execution:{status:'executing'}};if(command==='saved_job_status')return{...bound(),run_id:'native-run',phase:'stopped',outcome:'failed',execution:{status:'uncertain',outcome_recorded:false},message:'Outcome could not be recorded'};return bound();};
  try{
    const script=stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/job-runs.ts',import.meta.url),'utf8'));
    const service=await import(data(imports(script,{'@tauri-apps/api/core':data('export const invoke=(...args)=>globalThis.__executionRpc(...args);export class Channel{}'),'#lib/services/job-journal.js':data('export const noteJobJournalFailure=value=>globalThis.__executionNotice(value);'),'#lib/state/job-execution.svelte.js':data('export const jobExecutionState=globalThis.__executionShared;'),'#lib/state/navigation-cache.svelte.js':data('export const beginS3Mutation=()=>{globalThis.__executionCache("begin");return()=>globalThis.__executionCache("end");};')})));
    await assert.rejects(service.executeJobPreview('run','approval',false,true));await assert.rejects(service.executeJobPreview('run','approval',true,false));assert.equal(calls.length,0);await service.reviewJobExecution('native-run','native-preview');const result=await service.executeJobPreview('native-run','native-approval',true,true);
    assert.deepEqual(calls.map(c=>c[0]),['saved_job_execution_review','saved_job_execute','saved_job_status']);assert.deepEqual(calls[1][1],{request:{run_id:'native-run',approval_id:'native-approval',approved:true,acknowledge_exclusions:true}});assert.equal(shared.status,result);assert.equal(shared.status.execution.status,'uncertain');assert.ok(notices.some(n=>n.includes('Outcome could not be recorded')));
    globalThis.__executionRpc=async()=>{throw new Error('Native status unavailable');};await assert.rejects(service.executeJobPreview('native-run','consumed',true,true));assert.match(String(shared.error),/Native status unavailable/);assert.ok(notices.some(n=>String(n).includes('sent writes may have committed')));
    assert.deepEqual(cache,['begin','end','begin','end']);
  }finally{delete globalThis.__executionShared;delete globalThis.__executionNotice;delete globalThis.__executionRpc;delete globalThis.__executionCache;}
});
