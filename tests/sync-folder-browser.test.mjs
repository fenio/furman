import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';
const data=text=>`data:text/javascript;base64,${Buffer.from(text).toString('base64')}`;
const source=name=>readFileSync(new URL(`../src/lib/components/${name}.svelte`,import.meta.url),'utf8');
const folder=(name,path,extra={})=>({name,path,is_dir:true,is_symlink:false,...extra});
const deferred=()=>{let resolve;const promise=new Promise(ok=>{resolve=ok;});return{resolve,promise};};
function imports(code,dependencies){return code.replace(/from (['"])([^'"]+)\1/g,(_,quote,name)=>{const target=dependencies[name]??(name.startsWith('svelte')?import.meta.resolve(name):null);assert.ok(target,`Unmapped folder browser import ${name}`);return `from ${JSON.stringify(target)}`;});}
let serial=0;
async function fixture(location,methods={}){
  const key=`folderBrowse${serial++}`;const f={mounts:[],calls:[],chosen:[],props:{location,onChoose:path=>f.chosen.push(path),onCancel(){}},methods:{local:async path=>({path,entries:[]}),s3:async()=>({entries:[],next_token:null}),...methods}};globalThis[key]=f;
  const dependencies={svelte:data(`export const onMount=callback=>globalThis.${key}.mounts.push(callback);export const untrack=callback=>callback();`),'#lib/services/tauri.js':data(`export const listDirectory=(...args)=>{const f=globalThis.${key};f.calls.push(['local',...args]);return f.methods.local(...args);};`),'#lib/services/s3.js':data(`export const s3ListObjectsPage=(...args)=>{const f=globalThis.${key};f.calls.push(['s3',...args]);return f.methods.s3(...args);};`),'#lib/utils/sync-workflow.js':new URL('../src/lib/utils/sync-workflow.ts',import.meta.url).href};
  let script=source('SyncFolderBrowser').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1].replaceAll('$props()',`globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm,'');
  script+=`\nexport const harness={navigate,more,up,choose,type(value){draft=value;},get state(){return{current,draft,folders,busy,failure,pages,token,capped,canChoose};}};`;
  const code=compileModule(stripTypeScriptTypes(script),{generate:'client',filename:'FolderBrowser.js'}).js.code;const{harness:h}=await import(data(imports(code,dependencies)));let destroy;return{...f,h,mount(){destroy=f.mounts[0]();},close(){destroy?.();},cleanup(){delete globalThis[key];}};
}

test('local folder browser reads only explicitly mounted directory metadata and choosing returns a path, not transfer authority',async()=>{
  const f=await fixture({backend:'local',path:'/original',s3Id:''},{local:async path=>({path,entries:[folder('child',path+'/child'),folder('linked',path+'/linked',{is_symlink:true}),folder('file',path+'/file',{is_dir:false}),folder('..','/')]})});
  try{assert.equal(f.calls.length,0);f.h.choose();assert.equal(f.chosen.length,0);await f.h.navigate('/original');assert.deepEqual(f.calls,[['local','/original',true]]);assert.deepEqual(f.h.state.folders.map(entry=>entry.name),['child']);f.h.choose();assert.deepEqual(f.chosen,['/original']);await f.h.navigate('/original/child');f.h.choose();assert.equal(f.chosen.at(-1),'/original/child');assert.ok(!f.calls.some(call=>/copy|save|connect|scan|delete|execute/.test(call[0])));}finally{f.cleanup();}
});

test('typed-but-unopened folders cannot be mistaken for the confirmed browser location; validation never rewrites paths',async()=>{
  const f=await fixture({backend:'local',path:'/original',s3Id:''});try{await f.h.navigate('/original');f.h.type('/elsewhere');f.h.choose();assert.equal(f.chosen.length,0);await f.h.navigate('/elsewhere');f.h.choose();assert.deepEqual(f.chosen,['/elsewhere']);const before=f.calls.length;await f.h.navigate('/a/../b');assert.equal(f.calls.length,before);assert.ok(f.h.state.failure);}finally{f.cleanup();}
});

test('S3 browsing is connection/bucket scoped, paged on request, preserves prior folders on failure and retries the same page',async()=>{
  let second=0;const f=await fixture({backend:'s3',path:'s3://bucket/folder/',bucket:'bucket',s3Id:'native-connection'},{s3:async(id,prefix,token)=>{assert.equal(id,'native-connection');assert.equal(prefix,'folder/');if(token===null)return{entries:[folder('a','s3://bucket/folder/a/')],next_token:'page-2'};if(++second===1)throw new Error('AccessDenied');return{entries:[folder('b','s3://bucket/folder/b/')],next_token:null};}});
  try{await f.h.navigate('s3://bucket/folder/');assert.equal(f.calls.length,1);assert.equal(f.h.state.pages,1);await f.h.more();assert.equal(f.h.state.folders.length,1);assert.match(String(f.h.state.failure),/AccessDenied/);f.h.choose();assert.equal(f.chosen.length,0);await f.h.more();assert.equal(f.h.state.folders.length,2);assert.deepEqual(f.calls.slice(1).map(call=>call[3]),['page-2','page-2']);const before=f.calls.length;await f.h.navigate('s3://other/private/');assert.equal(f.calls.length,before);}finally{f.cleanup();}
});

test('repeated S3 cursors fail closed without losing accepted folders or guessing a complete scope',async()=>{
  const f=await fixture({backend:'s3',path:'s3://bucket/',bucket:'bucket',s3Id:'connection'},{s3:async()=>({entries:[folder('one','s3://bucket/one/')],next_token:'cycle'})});try{await f.h.navigate('s3://bucket/');await f.h.more();assert.equal(f.h.state.folders.length,1);assert.equal(f.h.state.pages,1);assert.match(String(f.h.state.failure),/continuation token/);assert.equal(f.h.state.canChoose,false);}finally{f.cleanup();}
});

test('parent navigation preserves literal S3 keys and cannot escape the selected bucket',async()=>{
  const f=await fixture({backend:'s3',path:'s3://bucket/literal%20name//child/',bucket:'bucket',s3Id:'connection'});
  try{await f.h.navigate('s3://bucket/literal%20name//child/');await f.h.up();assert.equal(f.calls.at(-1)[2],'literal%20name//');await f.h.navigate('s3://bucket/');await f.h.up();assert.equal(f.calls.at(-1)[2],'');assert.ok(f.calls.every(call=>call[1]==='connection'));f.h.choose();assert.deepEqual(f.chosen,['s3://bucket/']);}finally{f.cleanup();}
});

test('closing browser while metadata is pending discards late results and cannot change the chosen folder',async()=>{
  const pending=deferred();const f=await fixture({backend:'local',path:'/initial',s3Id:''},{local:async()=>pending.promise});try{f.mount();assert.equal(f.calls.length,1);f.close();pending.resolve({path:'/late',entries:[folder('late','/late')]});await Promise.resolve();await Promise.resolve();f.h.choose();assert.equal(f.chosen.length,0);assert.equal(f.h.state.current,'');}finally{f.cleanup();}
});

test('folder browsing caps visible folders and S3 page calls instead of silently enumerating a whole bucket',async()=>{
  const local=await fixture({backend:'local',path:'/initial',s3Id:''},{local:async()=>({path:'/initial',entries:Array.from({length:1001},(_,i)=>folder(String(i),`/initial/${i}`))})});try{await local.h.navigate('/initial');assert.equal(local.h.state.folders.length,1000);assert.equal(local.h.state.capped,true);}finally{local.cleanup();}
  let page=0;const s3=await fixture({backend:'s3',path:'s3://bucket/',bucket:'bucket',s3Id:'connection'},{s3:async()=>({entries:[],next_token:`page-${++page}`})});try{await s3.h.navigate('s3://bucket/');for(let i=0;i<30;i++)await s3.h.more();assert.equal(s3.calls.length,20);assert.equal(s3.h.state.pages,20);}finally{s3.cleanup();}
});

test('local-only picker offers Browse and both pane shortcuts, not a misleading one-option folder dropdown',async()=>{
  const code=compile(source('SyncLocationPicker'),{generate:'server',filename:'LocationPicker.svelte'}).js.code;const{default:Picker}=await import(data(imports(code,{'./SyncFolderBrowser.svelte':data('export default ()=>{};')})));const html=render(Picker,{props:{label:'Destination',value:{backend:'local',path:'/inherited',s3Id:''},locations:[{backend:'local',path:'/left',s3Id:'',label:'Left pane'},{backend:'local',path:'/right',s3Id:'',label:'Right pane'}],onChange(){}}}).body;assert.ok(html.includes('Browse folders…'));assert.ok(html.includes('Left pane: /left'));assert.ok(html.includes('Right pane: /right'));assert.ok(html.includes('not your only choices'));assert.ok(html.includes('No connected S3 bucket'));assert.ok(!html.includes('<select'));assert.ok(!html.includes('Reading folders'));
});
