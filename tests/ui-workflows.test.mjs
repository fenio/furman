import assert from 'node:assert/strict';
import test from 'node:test';
import { saveThenConnect } from '../src/lib/utils/connection-flow.ts';
import { defaultComparisonMode, canComparePanes, parseComparisonExcludes } from '../src/lib/utils/comparison-options.ts';
import { normalizeSseCustomerKey } from '../src/lib/utils/sse-c.ts';
import { textDiff, comparisonDiffPair } from '../src/lib/utils/file-diff.ts';
import { revealSecret } from '../src/lib/utils/reveal-secret.ts';
import { errorReport, errorText } from '../src/lib/utils/error-report.ts';
import { readFileSync } from 'node:fs';
import { compile } from 'svelte/compiler';
import { render } from 'svelte/server';
import { DEFAULT_SCAN_LIMITS, validScanLimits, scanLimitsFromConfig, scanSummary, canUseScan } from '../src/lib/utils/scan.ts';
import { validWritePolicy, writeNeedsApproval, syncWriteSelections } from '../src/lib/utils/guarded-writes.ts';

test('guarded writes require fresh previews and explicit replacement approval', () => {
  for (const policy of ['create_only', 'skip_existing', 'replace_unchanged', 'overwrite']) {
    assert.equal(validWritePolicy(policy, null), false);
    assert.equal(validWritePolicy(policy, { existing: 0, unguardable: 0 }), true);
  }
  assert.equal(validWritePolicy('create_only', { existing: 1, unguardable: 0 }), false);
  assert.equal(validWritePolicy('replace_unchanged', { existing: 1, unguardable: 1 }), false);
  assert.equal(validWritePolicy('skip_existing', { existing: 1, unguardable: 1 }), true);
  assert.equal(writeNeedsApproval('overwrite', 0), true);
  assert.equal(writeNeedsApproval('replace_unchanged', 1), true);
  assert.equal(writeNeedsApproval('replace_unchanged', 0), false);
  assert.equal(writeNeedsApproval('skip_existing', 1), false);
});

test('Sync write previews preserve nested paths, exclusions, hashes, and missing expectations', () => {
  const hash = 'a'.repeat(64);
  const selections = syncWriteSelections([
    { relative_path: 'nested/file', status: 'new', source_etag: hash, dest_etag: '' },
    { relative_path: 'other/file', status: 'modified', source_etag: hash, dest_etag: hash },
    { relative_path: 'deleted', status: 'deleted', source_etag: '', dest_etag: hash },
    { relative_path: 'metadata-only', status: 'modified', source_etag: '', dest_etag: '' },
  ], 's3://bucket/src', 'target/');
  assert.equal(selections.length, 3);
  assert.equal(selections[0].path, 's3://bucket/src/nested/file');
  assert.equal(selections[0].destination_key, 'target/nested/file');
  assert.deepEqual(selections[0].expected_destination, { type: 'missing' });
  assert.equal(selections[1].destination_key, 'target/other/file');
  assert.equal(selections[1].expected_sha256, hash);
  assert.equal(selections[1].expected_destination_sha256, hash);
  assert.equal(selections[1].expected_source_etag, undefined);
  assert.equal(selections[2].expected_sha256, undefined);
  assert.ok(!selections.some((selection) => selection.path.endsWith('/deleted')));
});

test('scan limits reject disabled, fractional, and excessive budgets and migrate old configs safely', () => {
  assert.equal(validScanLimits(DEFAULT_SCAN_LIMITS), true);
  for (const key of Object.keys(DEFAULT_SCAN_LIMITS)) {
    for (const invalid of [0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER]) {
      assert.equal(validScanLimits({ ...DEFAULT_SCAN_LIMITS, [key]: invalid }), false);
    }
  }
  assert.deepEqual(scanLimitsFromConfig(undefined), DEFAULT_SCAN_LIMITS);
  assert.deepEqual(scanLimitsFromConfig({ max_objects: 0 }), DEFAULT_SCAN_LIMITS);
  assert.deepEqual(scanLimitsFromConfig({ ...DEFAULT_SCAN_LIMITS, extra: 'not copied' }), DEFAULT_SCAN_LIMITS);
});

test('only exhaustive successful scans can authorize Sync', () => {
  for (const outcome of [null, 'cancelled', 'limited', 'failed', 'complete']) {
    assert.equal(canUseScan(outcome, true), false);
    assert.equal(canUseScan(outcome, false), outcome === 'complete');
  }
  const progress = { scanned: 20, matched: 3, requests: 2, bytes_read: 7, s3_bytes_read: 3, elapsed_ms: 2000 };
  assert.match(scanSummary(progress, 'limited'), /Incomplete — limited/);
  assert.match(scanSummary(progress, 'cancelled'), /Incomplete — cancelled/);
  assert.match(scanSummary(progress, 'failed'), /Incomplete — failed/);
  assert.match(scanSummary(progress, 'complete'), /^Complete:/);
});

test('scan status keeps stopped scans visibly incomplete and reports downloaded bytes', async () => {
  const { default: Status } = await import(serverComponent('ScanStatus.svelte', {
    '#lib/utils/scan.js': new URL('../src/lib/utils/scan.ts', import.meta.url).href,
    '#lib/utils/format.js': new URL('../src/lib/utils/format.ts', import.meta.url).href,
    './ErrorNotice.svelte': serverComponent('ErrorNotice.svelte'),
  }));
  for (const outcome of ['limited', 'cancelled', 'failed']) {
    const html = render(Status, { props: { outcome, progress: { scanned: 5, matched: 1, requests: 2, bytes_read: 7, s3_bytes_read: 3, elapsed_ms: 1000 }, message: 'A useful reason' } }).body;
    assert.ok(html.includes('Incomplete'));
    assert.ok(html.includes('cannot establish equality or authorize Sync'));
    assert.ok(html.includes('S3 payload read: 3'));
    assert.ok(html.includes('A useful reason'));
  }
});

function serverComponent(file, dependencies = {}) {
  const source = readFileSync(new URL(`../src/lib/components/${file}`, import.meta.url), 'utf8');
  const code = compile(source, { generate: 'server', filename: file }).js.code
    .replace(/from (['"])([^'"]+)\1/g, (_, quote, name) => {
      const resolved = dependencies[name] ?? (name.startsWith('svelte')
        ? import.meta.resolve(name)
        : name === '#lib/utils/error-report.js' ? new URL('../src/lib/utils/error-report.ts', import.meta.url).href : null);
      assert.ok(resolved, `Unmapped component import: ${name}`);
      return `from ${JSON.stringify(resolved)}`;
    });
  return `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
}

test('listing status distinguishes incomplete, failed, cancelled, and complete scopes', async () => {
  const { default: Status } = await import(serverComponent('S3ListingStatus.svelte', {
    './ErrorNotice.svelte': serverComponent('ErrorNotice.svelte'),
    '#lib/state/navigation-cache.svelte.js': 'data:text/javascript,export const navigationCacheState={revision:0};export const s3NavigationCache={limits:{ttlMs:60000}};',
  }));
  const progress = { status: 'loading', items: 10, pages: 1, requests: 2, elapsedMs: 2000, updatedAt: Date.now(), pending: true, error: null };
  const html = state => render(Status, { props: { progress: { ...progress, ...state }, onCancel() {}, onRetry() {}, onRefresh() {} } }).body;
  const loading = html({});
  assert.ok(loading.includes('loaded items'));
  assert.ok(loading.includes('all their children'));
  assert.ok(loading.includes('Cancel'));
  const cancelled = html({ status: 'cancelled' });
  assert.ok(cancelled.includes('Incomplete'));
  assert.ok(cancelled.includes('Continue'));
  assert.ok(cancelled.includes('disabled'));
  assert.ok(cancelled.includes('result will be discarded'));
  const failed = html({ status: 'error', pending: false, error: 'Listing page 2 failed\nHTTP 502' });
  assert.ok(failed.includes('Retry page'));
  assert.ok(failed.includes('HTTP 502'));
  const complete = html({ status: 'complete', pending: false });
  assert.ok(complete.includes('Complete'));
  assert.ok(!complete.includes('loaded items'));
});

test('cached listings disclose age, zero network calls, staleness, and manual refresh', async () => {
  const { default: Status } = await import(serverComponent('S3ListingStatus.svelte', {
    './ErrorNotice.svelte': serverComponent('ErrorNotice.svelte'),
    '#lib/state/navigation-cache.svelte.js': 'data:text/javascript,export const navigationCacheState={revision:2};export const s3NavigationCache={limits:{ttlMs:60000}};',
  }));
  const progress = { status: 'complete', items: 3, pages: 0, requests: 0, elapsedMs: 0, updatedAt: Date.now(), pending: false, error: null,
    source: 'cache', cachedAt: Date.now() - 61_000, fetchedAt: Date.now() - 61_000, cacheRevision: 1 };
  const html = render(Status, { props: { progress, onRefresh() {}, onCancel() {}, onRetry() {} } }).body;
  for (const text of ['Cached complete listing', 'no LIST calls for this visit', 'Stale view', 'invalidated this listing', 'Refresh']) assert.ok(html.includes(text), text);
  const uncached = render(Status, { props: { progress: { ...progress, source: 'network', cacheStored: false }, onRefresh() {}, onCancel() {}, onRetry() {} } }).body;
  assert.ok(uncached.includes('Not retained in navigation cache'));
});

test('error notices render escaped diagnostics with details closed initially', async () => {
  const { default: ErrorNotice } = await import(serverComponent('ErrorNotice.svelte'));
  const short = render(ErrorNotice, { props: { error: 'Permission denied' } }).body;
  assert.ok(short.includes('Permission denied'));
  assert.ok(!short.includes('<details'));
  const long = render(ErrorNotice, { props: { error: 'Listing failed\nHTTP 502\n<script>not executable</script>' } }).body;
  assert.match(long, /<details\b[^>]*>/);
  assert.doesNotMatch(long, /<details\b[^>]*\bopen(?:\s|=|>)/);
  assert.ok(long.includes('Copy error'));
  assert.ok(long.includes('HTTP 502'));
  assert.ok(!long.includes('<script>'));
  assert.ok(long.includes('&lt;script>'));
});

test('short errors stay inline; long and multiline errors retain complete details', () => {
  assert.equal(errorReport('Permission denied').details, '');
  const text = 'ListObjectsV2 failed\nHTTP 502\nrequestId: request-123';
  const report = errorReport(text, 'Could not list folder');
  assert.equal(report.summary, 'Could not list folder: ListObjectsV2 failed');
  assert.equal(report.details, text);
  assert.equal(report.copyText, `Could not list folder\n${text}`);
  const long = 'x'.repeat(4000);
  assert.equal(errorReport(long).summary.length, 198);
  assert.equal(errorReport(long).details, long);
});

test('error normalization retains causes and useful fields but not arbitrary data', () => {
  const cause = new Error('Connection reset');
  const error = new Error('Listing failed', { cause });
  assert.equal(errorText(error), 'Listing failed\nCaused by: Connection reset');
  assert.equal(errorText({ message: 'Forbidden', code: 'AccessDenied', status: 403, requestId: 'abc', secretKey: 'do-not-print', headers: { authorization: 'do-not-print' } }), 'Forbidden\ncode: AccessDenied\nstatus: 403\nrequestId: abc');
  cause.cause = error;
  assert.equal(errorText(error), 'Listing failed\nCaused by: Connection reset');
  for (const empty of [null, undefined, '', {}, { secretKey: 'hidden' }]) {
    assert.match(errorText(empty), /without additional error details/);
  }
});

test('text diffs identify added and removed lines with both line numbers', () => {
  const rows = textDiff('same\nold\nend\n', 'same\nnew\nend\n');
  assert.deepEqual(rows.map(row => [row.kind, row.left, row.right, row.text]), [
    ['same', 1, 1, 'same'], ['removed', 2, null, 'old'], ['added', null, 2, 'new'], ['same', 3, 3, 'end'],
  ]);
  assert.deepEqual(textDiff('', ''), []);
  assert.equal(textDiff('', 'new')[0].noNewline, true);
  assert.equal(textDiff('a\n', 'a')[1].noNewline, true);
  assert.throws(() => textDiff('\n'.repeat(10000), ''), /Too many lines/);
});

test('diff counterparts preserve nested paths and each connection identity', () => {
  const pair = comparisonDiffPair(['s3://one/prefix/', 's3', 'source-id', 's3://two/other/', 's3', 'dest-id'], 'nested/name.tfstate');
  assert.equal(pair.left.path, 's3://one/prefix/nested/name.tfstate');
  assert.equal(pair.right.path, 's3://two/other/nested/name.tfstate');
  assert.equal(pair.left.connection_id, 'source-id');
  assert.equal(pair.right.connection_id, 'dest-id');
  assert.equal(comparisonDiffPair(['/', 'local', '', '/other', 'local', ''], 'a').left.path, '/a');
});

test('credential eye toggles visibility without altering values or submitting', () => {
  const previousDocument = globalThis.document;
  const previousObserver = globalThis.MutationObserver;
  const created = [];
  let observed;
  class Element {
    type = 'password'; value = '  original-secret  '; disabled = false;
    style = {}; attributes = {}; events = {}; children = [];
    before() {} append(child) { this.children.push(child); } remove() {}
    setAttribute(name, value) { this.attributes[name] = value; }
    addEventListener(name, callback) { this.events[name] = callback; }
    removeEventListener(name) { delete this.events[name]; }
    focus() { this.focused = true; }
  }
  globalThis.document = { createElement() { const node = new Element(); created.push(node); return node; } };
  globalThis.MutationObserver = class { constructor(callback) { observed = callback; } observe() {} disconnect() {} };
  try {
    const input = new Element();
    const action = revealSecret(input);
    const button = created[1];
    assert.equal(button.type, 'button');
    assert.equal(button.attributes['aria-pressed'], 'false');
    button.events.click();
    assert.equal(input.type, 'text');
    assert.equal(input.value, '  original-secret  ');
    assert.equal(button.attributes['aria-pressed'], 'true');
    input.disabled = true; observed(); assert.equal(button.disabled, true);
    action.destroy(); assert.equal(input.type, 'password');
    const count = created.length;
    revealSecret(new Element(), false);
    assert.equal(created.length, count);
  } finally {
    globalThis.document = previousDocument;
    globalThis.MutationObserver = previousObserver;
  }
});

test('Save & Connect waits for saving before attempting a connection', async () => {
  const events = [];
  let finishSave;
  const pending = saveThenConnect(() => new Promise(resolve => { finishSave = resolve; events.push('saving'); }), async () => { events.push('connecting'); });
  assert.deepEqual(events, ['saving']);
  finishSave();
  await pending;
  assert.deepEqual(events, ['saving', 'connecting']);
});

test('a failed save prevents connection', async () => {
  let connected = false;
  await assert.rejects(saveThenConnect(async () => { throw new Error('keychain failed'); }, () => { connected = true; }), /keychain failed/);
  assert.equal(connected, false);
});

test('connection errors are not swallowed by the save/connect sequence', async () => {
  await assert.rejects(saveThenConnect(() => {}, async () => { throw new Error('connection failed'); }), /connection failed/);
});

test('any S3 pane defaults to content verification', () => {
  assert.equal(defaultComparisonMode('s3', 's3'), 'sha256');
  assert.equal(defaultComparisonMode('s3', 'local'), 'sha256');
  assert.equal(defaultComparisonMode('local', 's3'), 'sha256');
  assert.equal(defaultComparisonMode('local', 'local'), 'size_mtime');
});

test('only supported pane backend pairs can start comparison', () => {
  for (const left of ['local', 's3', 'sftp', 'archive']) {
    for (const right of ['local', 's3', 'sftp', 'archive']) {
      assert.equal(canComparePanes(left, right), ['local', 's3'].includes(left) && ['local', 's3'].includes(right));
    }
  }
});

test('exclusions are explicit and never silently added', () => {
  assert.deepEqual(parseComparisonExcludes(''), []);
  assert.deepEqual(parseComparisonExcludes('  .DS_Store, *.tmp, ,node_modules/**  '), ['.DS_Store', '*.tmp', 'node_modules/**']);
});

test('SSE-C key formats normalize to the same base64 key', () => {
  const raw = '0123456789abcdef0123456789abcdef';
  const key = Buffer.from(raw).toString('base64');
  assert.equal(normalizeSseCustomerKey(key), key);
  assert.equal(normalizeSseCustomerKey(raw, 'raw'), key);
  assert.equal(normalizeSseCustomerKey(Buffer.from(raw).toString('hex'), 'hex'), key);
});

test('invalid SSE-C keys fail without echoing the secret', () => {
  for (const [input, format] of [['not-a-key!', 'base64'], ['shortsecret', 'raw'], ['001122', 'hex']]) {
    assert.throws(() => normalizeSseCustomerKey(input, format), error => error.message.includes('32-byte') && !error.message.includes(input));
  }
});
