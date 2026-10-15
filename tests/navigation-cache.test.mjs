import assert from 'node:assert/strict';
import test from 'node:test';
import { NavigationCache, mayChangeS3 } from '../src/lib/utils/navigation-cache.ts';
import { activityExport } from '../src/lib/utils/activity-export.ts';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';

const row = (path) => ({ path, name: path, size: 3 });

test('cache is connection/prefix scoped, copies flat rows, and never slides TTL on reads', () => {
  let now = 0;
  const cache = new NavigationCache({ views: 2, entries: 3, weight: 4096, ttlMs: 100 }, () => now);
  const entries = [row('file')];
  assert.equal(cache.put('connection-a/prefix', entries, 0), true);
  entries[0].name = 'changed';
  assert.equal(cache.get('connection-a/prefix').entries[0].name, 'file');
  const fetched = cache.get('connection-a/prefix'); fetched.entries[0].name = 'changed-again';
  assert.equal(cache.get('connection-a/prefix').entries[0].name, 'file');
  assert.equal(cache.get('connection-b/prefix'), null);
  now = 99; assert.ok(cache.get('connection-a/prefix'));
  now = 100; assert.equal(cache.get('connection-a/prefix'), null);
  assert.deepEqual(cache.stats(), { views: 0, entries: 0, estimatedBytes: 0, revision: 0, pendingMutations: 0 });
});

test('LRU eviction obeys both view and aggregate entry limits', () => {
  const cache = new NavigationCache({ views: 2, entries: 3, weight: 4096, ttlMs: 100 }, () => 0);
  cache.put('a', [row('a')], 0); cache.put('b', [row('b')], 0); cache.get('a');
  cache.put('c', [row('c')], 0);
  assert.equal(cache.get('b'), null); assert.ok(cache.get('a'));
  cache.put('d', [row('d1'), row('d2')], 0);
  assert.equal(cache.get('c'), null); assert.equal(cache.stats().entries, 3);
  assert.equal(cache.put('too-large', Array.from({ length: 4 }, (_, i) => row(String(i))), 0), false);
});

test('byte admission budget includes long names, not just entry counts', () => {
  const cache = new NavigationCache({ views: 3, entries: 20, weight: 1100, ttlMs: 100 }, () => 0);
  assert.equal(cache.put('a', [row('a')], 0), true);
  assert.equal(cache.put('b', [row('b')], 0), true);
  assert.equal(cache.put('long', [row('x'.repeat(1000))], 0), false);
  assert.ok(cache.stats().estimatedBytes <= 1100);
  cache.put('c', [row('c')], 0);
  assert.equal(cache.get('a'), null);
});

test('refreshes, reconnects, and overlapping mutations block stale repopulation', () => {
  const cache = new NavigationCache();
  const revision = cache.revision;
  cache.put('a', [row('a')], revision);
  const endFirst = cache.beginMutation();
  const duringWrite = cache.revision;
  assert.equal(cache.get('a'), null);
  assert.equal(cache.put('a', [row('stale')], duringWrite), false);
  const endSecond = cache.beginMutation();
  endFirst(); endFirst(); // An accidental duplicate completion cannot end another write.
  assert.equal(cache.stats().pendingMutations, 1);
  assert.equal(cache.put('a', [row('stale')], cache.revision), false);
  endSecond();
  assert.equal(cache.put('a', [row('stale')], revision), false);
  assert.equal(cache.put('a', [row('stale')], duringWrite), false);
  assert.equal(cache.put('a', [row('fresh')], cache.revision), true);
  cache.clear(); assert.equal(cache.get('a'), null);
});

test('unknown and mutating commands invalidate conservatively, reads do not', () => {
  for (const command of ['s3_upload', 's3_upload_encrypted', 's3_copy_objects', 's3_put_text', 's3_execute_write', 's3_rename_object', 's3_delete_objects', 's3_connect', 's3_disconnect', 's3_restore_version', 's3_put_object_metadata', 'future_write_command']) {
    assert.equal(mayChangeS3(command), true, command);
  }
  for (const command of ['s3_list_objects_page', 's3_get_object_tags', 's3_head_object', 's3_prepare_write', 's3_discard_write', 's3_download', 's3_presign_url', 's3_search_objects', 's3_activity_snapshot', 's3_activity_clear']) {
    assert.equal(mayChangeS3(command), false, command);
  }
});

test('diagnostic export excludes arbitrary fields, connection settings, and payloads', () => {
  const record = { id: 1, started_ms: 2, operation: 'PutObject', duration_ms: 3, outcome: 'success', http_status: 200, request_id: 'request-1', attempts: 2, retries: 1, authorization: 'SECRET', key: 'PRIVATE', headers: { 'sse-c': 'SECRET' } };
  const exported = activityExport({ schema_version: 1, app_version: 'test', capacity: 500, evicted: 0, records: [record], profile: 'PRIVATE', password: 'SECRET', payload: 'PRIVATE' });
  assert.ok(exported.includes('PutObject')); assert.ok(exported.includes('request-1'));
  assert.ok(!exported.includes('SECRET')); assert.ok(!exported.includes('PRIVATE'));
  assert.equal(JSON.parse(exported).records[0].retries, 1);
});

test('actual S3 service invalidates before writes and again on rejection, without caching preflights', async () => {
  const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
  let rejectWrite;
  const pending = new Promise((_resolve, reject) => { rejectWrite = reject; });
  const cache = new NavigationCache();
  globalThis.__s3ServiceCacheTest = { cache, invoke: command => command === 's3_put_text' ? pending : Promise.resolve({}) };
  const dependencies = {
    '@tauri-apps/api/core': data('export const invoke=(...args)=>globalThis.__s3ServiceCacheTest.invoke(...args);export class Channel {}'),
    '#lib/state/navigation-cache.svelte.js': data('export const beginS3Mutation=()=>globalThis.__s3ServiceCacheTest.cache.beginMutation();'),
    '#lib/utils/navigation-cache.js': new URL('../src/lib/utils/navigation-cache.ts', import.meta.url).href,
  };
  const source = stripTypeScriptTypes(readFileSync(new URL('../src/lib/services/s3.ts', import.meta.url), 'utf8'))
    .replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
      assert.ok(dependencies[name], name); return `from ${JSON.stringify(dependencies[name])}`;
    });
  try {
    const service = await import(data(source));
    cache.put('scope', [row('old')], cache.revision);
    const original = cache.revision;
    await service.s3HeadObject('connection', 'key');
    await service.s3PrepareWrite('op', null, 'connection', [{ path: '/file' }], '', () => {});
    assert.equal(cache.revision, original);
    assert.ok(cache.get('scope'));
    const writing = service.s3PutText('connection', 'key', 'payload');
    assert.equal(cache.get('scope'), null);
    assert.equal(cache.stats().pendingMutations, 1);
    const during = cache.revision;
    assert.equal(cache.put('scope', [row('stale')], during), false);
    rejectWrite(new Error('uncertain write failure'));
    await assert.rejects(writing, /uncertain write failure/);
    assert.equal(cache.stats().pendingMutations, 0);
    assert.ok(cache.revision > during);
    assert.equal(cache.put('scope', [row('stale')], original), false);
    assert.equal(cache.put('scope', [row('stale')], during), false);
  } finally { delete globalThis.__s3ServiceCacheTest; }
});
