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
