import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import { showAwsProfileTools, allowSsoLogin, isSsoSessionError } from '../src/lib/utils/aws-sso.ts';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;

test('AWS SSO controls never appear for Linode/Akamai, custom endpoints, anonymous or OIDC auth', () => {
  assert.equal(showAwsProfileTools('aws', ''), true);
  assert.equal(showAwsProfileTools(undefined, undefined), true);
  for (const provider of ['linode', 'akamai', 'minio', 'r2', 'custom', 'garage']) assert.equal(showAwsProfileTools(provider, ''), false, provider);
  assert.equal(showAwsProfileTools('aws', 'https://us-ord-1.linodeobjects.com'), false);
  assert.equal(showAwsProfileTools('aws', '', true), false);
  assert.equal(showAwsProfileTools('aws', '', false, true), false);
  assert.equal(allowSsoLogin('LINODE_ACCESS', 'SECRET'), false);
  assert.equal(allowSsoLogin('', 'SECRET'), false);
  assert.equal(allowSsoLogin('', ''), true);
  for (const code of ['AccessDenied', 'ExpiredToken', 'InvalidAccessKeyId', 'SignatureDoesNotMatch']) assert.equal(isSsoSessionError(`HTTP 403 ${code}`), false);
  assert.equal(isSsoSessionError('[AWS_SSO_SESSION_EXPIRED] reconnect'), true);
});

function rewriteImports(source, dependencies) {
  return source.replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
    const resolved = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(resolved, `Unmapped import: ${name}`); return `from ${JSON.stringify(resolved)}`;
  });
}

test('Linode/Akamai RPC keeps endpoint, region, manual keys, proxy and SSE-C arguments unchanged', async () => {
  const calls = []; globalThis.__awsSsoTest = { invoke: async (command, args) => { calls.push({ command, args }); } };
  const source = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/s3.ts', import.meta.url), 'utf8'));
  const dependencies = {
    '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__awsSsoTest.invoke(...args);export class Channel{}'),
    '#lib/state/navigation-cache.svelte.js': data('export const beginS3Mutation=()=>()=>{};'),
    '#lib/utils/navigation-cache.js': new URL('../src/lib/utils/navigation-cache.ts', import.meta.url).href,
  };
  try {
    const service = await import(data(rewriteImports(source, dependencies)));
    const args = ['linode-id', 'bucket', 'us-ord-1', 'https://us-ord-1.linodeobjects.com', undefined, 'LINODE_ACCESS', 'SECRET'];
    args[14] = 'http://proxy.example'; args[15] = 'proxy-user'; args[16] = 'proxy-password'; args[17] = 'FAKE_SSE_C_KEY'; args[18] = 'linode';
    await service.s3Connect(...args);
    assert.equal(calls.length, 1); assert.equal(calls[0].command, 's3_connect');
    const sent = calls[0].args;
    assert.equal(sent.endpoint, args[3]); assert.equal(sent.region, args[2]); assert.equal(sent.accessKey, args[5]); assert.equal(sent.secretKey, args[6]);
    assert.equal(sent.proxyUrl, args[14]); assert.equal(sent.proxyUsername, args[15]); assert.equal(sent.proxyPassword, args[16]); assert.equal(sent.sseCustomerKey, args[17]); assert.equal(sent.provider, 'linode');
    assert.ok(!calls.some(call => call.command.startsWith('aws_')));
  } finally { delete globalThis.__awsSsoTest; }
});

test('SSO service requires approval and never connects, retries, imports keys or stores tokens', async () => {
  const calls = []; let clears = 0;
  globalThis.__awsSsoServiceTest = { invoke: async (command, args) => { calls.push({ command, args }); if (command === 'aws_sso_login') throw new Error('CLI exit 53'); return { profiles: [], default_profile: 'default' }; }, clear: () => { clears++; } };
  const source = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/aws.ts', import.meta.url), 'utf8'));
  const dependencies = {
    '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__awsSsoServiceTest.invoke(...args);'),
    '#lib/state/navigation-cache.svelte.js': data('export const clearS3NavigationCache=()=>globalThis.__awsSsoServiceTest.clear();'),
  };
  try {
    const service = await import(data(rewriteImports(source, dependencies)));
    await service.awsSharedProfiles(); assert.equal(clears, 0);
    await assert.rejects(service.awsSsoLogin('job', 'selected', 'source', false), /approval/);
    assert.equal(calls.length, 1);
    await assert.rejects(service.awsSsoLogin('job', 'selected', 'source', true), /CLI exit 53/);
    assert.deepEqual(calls[1], { command: 'aws_sso_login', args: { opId: 'job', profile: 'selected', loginProfile: 'source', approved: true } });
    assert.equal(clears, 2); assert.equal(calls.length, 2);
  } finally { delete globalThis.__awsSsoServiceTest; }
});

function component(file, dependencies) {
  const source = readFileSync(new URL(`../src/lib/components/${file}`, import.meta.url), 'utf8');
  return data(rewriteImports(compile(source, { generate: 'server', filename: file }).js.code, dependencies));
}

test('actual connection form leaves Linode keys visible and does not render AWS profile tools', async () => {
  const common = {
    '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
    '#lib/utils/aws-sso.js': new URL('../src/lib/utils/aws-sso.ts', import.meta.url).href,
    '#lib/services/aws.js': data('export const awsSharedProfiles=async()=>{throw new Error("No SSR discovery");};export const awsSsoLogin=async()=>{throw new Error("No auto login");};export const awsSsoCancel=async()=>false;'),
  };
  const notice = component('ErrorNotice.svelte', common);
  const tools = component('AwsProfileTools.svelte', { ...common, './ErrorNotice.svelte': notice });
  const dependencies = {
    ...common, './ErrorNotice.svelte': notice, './AwsProfileTools.svelte': tools,
    '#lib/utils/reveal-secret.js': new URL('../src/lib/utils/reveal-secret.ts', import.meta.url).href,
    '#lib/utils/connection-flow.js': new URL('../src/lib/utils/connection-flow.ts', import.meta.url).href,
    '#lib/utils/sse-c.js': new URL('../src/lib/utils/sse-c.ts', import.meta.url).href,
    '#lib/state/connections.svelte.js': data('export const connectionsState={};'),
    '#lib/services/s3.js': data('export const s3CheckCredentials=async()=>false;export const s3ListBuckets=async()=>[];export const s3CreateBucket=async()=>{};export const s3DeleteBucket=async()=>{};export const oidcStartAuth=async()=>{};'),
    '#lib/data/s3-providers.js': data('export const S3_PROVIDERS=[];export const getProvider=id=>({id,name:id,icon:"",regions:[],capabilities:{listBuckets:true}});export const inferProviderFromEndpoint=()=>"linode";'),
  };
  const { default: Dialog } = await import(component('S3ConnectDialog.svelte', dependencies));
  const linode = { type: 's3', id: 'saved', name: 'Linode', bucket: 'bucket', region: 'us-ord-1', endpoint: 'https://us-ord-1.linodeobjects.com', credentialType: 'keychain', provider: 'linode', accessKeyId: 'LINODE_ACCESS', sseCustomerKeyEnabled: true };
  const html = render(Dialog, { props: { initialData: linode, onConnect() {}, onCancel() {} } }).body;
  assert.ok(html.includes('LINODE_ACCESS')); assert.ok(html.includes('us-ord-1.linodeobjects.com'));
  assert.ok(!html.includes('data-aws-profile-tools')); assert.ok(!html.includes('Sign in with AWS SSO'));
  const aws = render(Dialog, { props: { onConnect() {}, onCancel() {} } }).body;
  assert.ok(aws.includes('data-aws-profile-tools')); assert.ok(aws.includes('Discovery reads local AWS config'));
});
