import assert from 'node:assert/strict';
import test from 'node:test';
import { summarizeVersionSizes } from '../src/lib/utils/s3-versions.ts';

const version = (size, options = {}) => ({
  version_id: 'version', is_latest: false, is_delete_marker: false,
  size, modified: 0, etag: null, storage_class: null, ...options,
});

test('sums current and previous payload sizes, excluding delete markers', () => {
  const summary = summarizeVersionSizes([
    version(100, { is_latest: true }), version(200), version(300),
    version(999, { is_delete_marker: true }),
  ]);
  assert.deepEqual(summary, {
    currentBytes: 100n, previousBytes: 500n, totalBytes: 600n,
    dataVersionCount: 3, previousVersionCount: 2, deleteMarkerCount: 1,
  });
});

test('a latest delete marker leaves all retained payloads in history', () => {
  const summary = summarizeVersionSizes([
    version(0, { is_latest: true, is_delete_marker: true }), version(100),
  ]);
  assert.equal(summary.currentBytes, 0n);
  assert.equal(summary.previousBytes, 100n);
  assert.equal(summary.totalBytes, 100n);
});

test('counts null versions, zero-byte versions and identical copies separately', () => {
  const summary = summarizeVersionSizes([
    version(0, { is_latest: true }), version(100, { version_id: 'null' }), version(100),
  ]);
  assert.equal(summary.totalBytes, 200n);
  assert.equal(summary.dataVersionCount, 3);
});

test('empty history produces zero totals', () => {
  assert.equal(summarizeVersionSizes([]).totalBytes, 0n);
  assert.equal(summarizeVersionSizes([]).dataVersionCount, 0);
});

test('totals remain exact beyond JavaScript safe-integer range', () => {
  const summary = summarizeVersionSizes([
    version(Number.MAX_SAFE_INTEGER, { is_latest: true }), version(2),
  ]);
  assert.equal(summary.totalBytes, 9007199254740993n);
});

test('a refreshed listing updates totals after deletion and restoration', () => {
  assert.equal(summarizeVersionSizes([version(3, { is_latest: true }), version(5)]).totalBytes, 8n);
  assert.equal(summarizeVersionSizes([version(5, { is_latest: true })]).totalBytes, 5n);
  assert.equal(summarizeVersionSizes([version(5, { is_latest: true }), version(5)]).totalBytes, 10n);
});
