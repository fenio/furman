import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compileModule } from 'svelte/compiler';
import { NavigationCache } from '../src/lib/utils/navigation-cache.ts';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const noops = names => data(names.split(' ').map(name => `export const ${name}=async()=>{};`).join(''));
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const row = name => ({ path: `s3://bucket/${name}`, name, is_dir: false, size: 1, modified: 0 });
let fixtureId = 0;

async function panelFixture(fetch) {
  const key = `panelCacheFixture${fixtureId++}`;
  const fixture = { cache: new NavigationCache(), fetch, onStored: () => {} };
  globalThis[key] = fixture;
  const put = fixture.cache.put.bind(fixture.cache);
  fixture.cache.put = (...args) => { const result = put(...args); fixture.onStored(result); return result; };
  const dependencies = {
    '#lib/utils/error-report.js': new URL('../src/lib/utils/error-report.ts', import.meta.url).href,
    '#lib/utils/sort.js': new URL('../src/lib/utils/sort.ts', import.meta.url).href,
    '#lib/utils/progressive-listing.js': new URL('../src/lib/utils/progressive-listing.ts', import.meta.url).href,
    '#lib/services/tauri.js': noops('listDirectory listDirectoryStreamed listArchive watchDirectory unwatchDirectory getGitRepoInfo getDirectorySize cleanupTempPath'),
    '#lib/services/sftp.js': noops('sftpConnect sftpDisconnect sftpListObjects'),
    '#lib/services/mount.js': data('export const mountNetworkShare=async()=>{};'),
    '#lib/services/s3.js': data(`export const s3Connect=async()=>{};export const s3Disconnect=async()=>{};export const s3IsObjectEncrypted=async()=>{};export const s3ListObjectsPage=(...args)=>globalThis[${JSON.stringify(key)}].fetch(...args);`),
    '#lib/state/app.svelte.js': data('export const appState={sortField:"name",sortDirection:"asc",defaultViewMode:"list"};'),
    '#lib/state/connections.svelte.js': data('export const connectionsState={};'),
    '#lib/state/comparison.svelte.js': data('export const comparisonState={active:false};'),
    '#lib/state/navigation-cache.svelte.js': data(`export const s3NavigationCache=globalThis[${JSON.stringify(key)}].cache;export const clearS3NavigationCache=()=>s3NavigationCache.clear();`),
  };
  const source = stripTypeScriptTypes(readFileSync(new URL('../src/lib/state/panels.svelte.ts', import.meta.url), 'utf8'));
  const compiled = compileModule(source, { generate: 'client', filename: 'panels.svelte.js' }).js.code
    .replace(/from (['"])([^'"]+)\1/g, (_, quote, name) => {
      const resolved = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
      assert.ok(resolved, `Unmapped panel import: ${name}`);
      return `from ${JSON.stringify(resolved)}`;
    });
  const { PanelData } = await import(data(compiled));
  const panel = new PanelData('left');
  panel.backend = 's3'; panel.s3Connection = { connectionId: 'connection', bucket: 'bucket' };
  return { ...fixture, panel, stored: () => { const next = deferred(); fixture.onStored = next.resolve; return next.promise; }, cleanup: () => { panel.disposeS3Listing(); delete globalThis[key]; } };
}

test('actual panel navigation reuses complete views but same-path Refresh always fetches', async () => {
  const calls = [];
  const fixture = await panelFixture(async (id, prefix) => { calls.push([id, prefix]); return { entries: [row(prefix + 'file')], next_token: null }; });
  try {
    let stored = fixture.stored(); await fixture.panel.loadDirectory('s3://bucket/a/'); await stored;
    stored = fixture.stored(); await fixture.panel.loadDirectory('s3://bucket/b/'); await stored;
    fixture.panel.goBack();
    assert.equal(calls.length, 2);
    assert.equal(fixture.panel.s3ListingProgress.source, 'cache');
    assert.equal(fixture.panel.s3ListingProgress.requests, 0);
    fixture.panel.goForward();
    assert.equal(fixture.panel.path, 's3://bucket/b/');
    assert.equal(calls.length, 2);
    fixture.panel.goBack();
    stored = fixture.stored(); await fixture.panel.loadDirectory(fixture.panel.path); await stored;
    assert.equal(calls.length, 3);
    assert.equal(fixture.panel.s3ListingProgress.source, 'network');
  } finally { fixture.cleanup(); }
});

test('actual panel never caches partial/cancelled listings or late results after invalidation', async () => {
  const second = deferred(); const started = deferred();
  const fixture = await panelFixture(async (_id, _prefix, token) => {
    if (token === null) return { entries: [row('partial')], next_token: 'next' };
    started.resolve(); return second.promise;
  });
  try {
    await fixture.panel.loadDirectory('s3://bucket/a/'); await started.promise;
    assert.equal(fixture.panel.entries.length, 1);
    assert.equal(fixture.cache.stats().views, 0);
    fixture.panel.cancelS3Listing(); second.resolve({ entries: [row('late')], next_token: null });
    await new Promise(setImmediate);
    assert.equal(fixture.cache.stats().views, 0);
    assert.equal(fixture.panel.s3ListingProgress.status, 'cancelled');
    assert.equal(fixture.panel.entries.length, 1);
  } finally { fixture.cleanup(); }

  const result = deferred(); const late = await panelFixture(() => result.promise);
  try {
    const stored = late.stored(); await late.panel.loadDirectory('s3://bucket/a/');
    late.cache.clear(); result.resolve({ entries: [row('late')], next_token: null });
    assert.equal(await stored, false);
    assert.equal(late.cache.stats().views, 0);
  } finally { late.cleanup(); }
});

test('actual panel does not reuse another connection or cache failed prefixes', async () => {
  let calls = 0;
  const fixture = await panelFixture(async () => { calls++; return { entries: [row('file')], next_token: null }; });
  try {
    let stored = fixture.stored(); await fixture.panel.loadDirectory('s3://bucket/a/'); await stored;
    stored = fixture.stored(); await fixture.panel.loadDirectory('s3://bucket/b/'); await stored;
    fixture.panel.s3Connection = { connectionId: 'different-profile-revision', bucket: 'bucket' };
    stored = fixture.stored(); await fixture.panel.loadDirectory('s3://bucket/a/'); await stored;
    assert.equal(calls, 3);
    assert.equal(fixture.panel.s3ListingProgress.source, 'network');
  } finally { fixture.cleanup(); }
  const failed = await panelFixture(async () => { throw new Error('provider failure'); });
  try {
    await failed.panel.loadDirectory('s3://bucket/a/'); await new Promise(setImmediate);
    assert.equal(failed.cache.stats().views, 0);
    assert.equal(failed.panel.s3ListingProgress.status, 'error');
    assert.ok(failed.panel.s3ListingProgress.error.includes('provider failure'));
  } finally { failed.cleanup(); }
});
