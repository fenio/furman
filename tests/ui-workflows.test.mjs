import assert from 'node:assert/strict';
import test from 'node:test';
import { saveThenConnect } from '../src/lib/utils/connection-flow.ts';
import { defaultComparisonMode, canComparePanes, parseComparisonExcludes } from '../src/lib/utils/comparison-options.ts';
import { normalizeSseCustomerKey } from '../src/lib/utils/sse-c.ts';

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
