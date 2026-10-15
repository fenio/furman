import assert from 'node:assert/strict';
import test from 'node:test';
import { ProgressiveListing, listingPosition } from '../src/lib/utils/progressive-listing.ts';
import { sortEntries } from '../src/lib/utils/sort.ts';

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const item = name => ({ path: `s3://bucket/${name}`, name });

test('arriving sorted pages preserve cursor and range anchor by path', () => {
  const first = [{ ...item('z'), size: 3 }, { ...item('m'), size: 2 }];
  const cursorPath = first[0].path, anchorPath = first[1].path;
  const loaded = sortEntries([...first, { ...item('a'), size: 1 }], 'name', 'asc');
  assert.deepEqual(listingPosition(loaded, cursorPath, anchorPath), { cursorIndex: 2, selectionAnchor: 1, focused: false });
  assert.deepEqual(listingPosition(loaded, cursorPath, anchorPath, 'a'), { cursorIndex: 0, selectionAnchor: 1, focused: true });
  assert.deepEqual(listingPosition([], cursorPath), { cursorIndex: 0, selectionAnchor: 0, focused: false });
});

test('first-page results are published before a slow second page finishes', async () => {
  const secondStarted = deferred();
  const second = deferred();
  const pages = [];
  const progress = [];
  const listing = new ProgressiveListing(async token => {
    if (token === null) return { entries: [item('..'), item('first')], next_token: 'next' };
    assert.equal(token, 'next');
    secondStarted.resolve();
    return await second.promise;
  }, entries => pages.push(entries), state => progress.push(state));
  const done = listing.start();
  await secondStarted.promise;
  assert.deepEqual(pages.flat().map(e => e.name), ['..', 'first']);
  assert.equal(progress.at(-1).status, 'loading');
  assert.equal(progress.at(-1).items, 1);
  second.resolve({ entries: [item('last')], next_token: null });
  await done;
  assert.deepEqual(pages.flat().map(e => e.name), ['..', 'first', 'last']);
  assert.equal(progress.at(-1).status, 'complete');
  assert.equal(progress.at(-1).pages, 2);
  assert.equal(progress.at(-1).requests, 2);
  assert.equal(progress.at(-1).pending, false);
});

test('failed pages preserve accepted entries and retry from the same cursor', async () => {
  const tokens = [], entries = [], states = [];
  let fail = true;
  const listing = new ProgressiveListing(async token => {
    tokens.push(token);
    if (!token) return { entries: [item('first')], next_token: 'page-two' };
    if (fail) throw new Error('HTTP 502\nrequestId: request-123');
    return { entries: [item('first'), item('second')], next_token: null };
  }, page => entries.push(...page), state => states.push(state));
  await listing.start();
  assert.deepEqual(entries.map(e => e.name), ['first']);
  assert.equal(states.at(-1).status, 'error');
  assert.match(states.at(-1).error, /page 2 failed\nHTTP 502/);
  fail = false;
  await listing.start();
  assert.deepEqual(tokens, [null, 'page-two', 'page-two']);
  assert.deepEqual(entries.map(e => e.name), ['first', 'second']);
  assert.equal(states.at(-1).items, 2);
  assert.equal(states.at(-1).status, 'complete');
});

test('cancel discards an in-flight page and prevents overlap; continue resumes it', async () => {
  const started = deferred(), response = deferred();
  const entries = [], states = [], tokens = [];
  let stall = true;
  const listing = new ProgressiveListing(async token => {
    tokens.push(token);
    if (!token) return { entries: [item('first')], next_token: 'two' };
    if (stall) { started.resolve(); return response.promise; }
    return { entries: [item('second')], next_token: null };
  }, page => entries.push(...page), state => states.push(state));
  const done = listing.start();
  await started.promise;
  listing.cancel();
  await listing.start();
  assert.equal(tokens.length, 2);
  assert.equal(states.at(-1).status, 'cancelled');
  response.resolve({ entries: [item('discarded')], next_token: 'three' });
  await done;
  assert.deepEqual(entries.map(e => e.name), ['first']);
  assert.equal(states.at(-1).pending, false);
  stall = false;
  await listing.start();
  assert.deepEqual(tokens, [null, 'two', 'two']);
  assert.deepEqual(entries.map(e => e.name), ['first', 'second']);
});

test('disposed navigation/tab sessions cannot publish late successes or errors', async () => {
  for (const fail of [false, true]) {
    const response = deferred(), entries = [], states = [];
    const listing = new ProgressiveListing(() => response.promise, page => entries.push(...page), state => states.push(state));
    const done = listing.start();
    listing.dispose();
    const count = states.length;
    if (fail) response.reject(new Error('old folder failed'));
    else response.resolve({ entries: [item('old')], next_token: null });
    await done;
    assert.equal(entries.length, 0);
    assert.equal(states.length, count);
  }
});

test('cyclic pagination fails closed without committing the invalid page', async () => {
  const entries = [], states = [];
  const cursors = ['a', 'b', 'a'];
  const listing = new ProgressiveListing(async () => ({ entries: [item(String(cursors.length))], next_token: cursors.shift() }), page => entries.push(...page), state => states.push(state));
  await listing.start();
  assert.equal(entries.length, 2);
  assert.equal(states.at(-1).pages, 2);
  assert.equal(states.at(-1).status, 'error');
  assert.match(states.at(-1).error, /continuation token/);
});

test('empty final pages complete, and completed listings cannot restart accidentally', async () => {
  let calls = 0;
  const states = [];
  const listing = new ProgressiveListing(async () => { calls++; return { entries: [], next_token: null }; }, () => {}, state => states.push(state));
  await listing.start();
  await listing.start();
  assert.equal(calls, 1);
  assert.equal(states.at(-1).status, 'complete');
  assert.equal(states.at(-1).items, 0);
});
