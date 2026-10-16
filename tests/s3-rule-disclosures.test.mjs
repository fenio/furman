import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';
import { initialExpandedRule, expandedRuleAfterRemoval } from '../src/lib/utils/rule-disclosure.ts';

const source = readFileSync(new URL('../src/lib/components/PropertiesDialog.svelte', import.meta.url), 'utf8');
const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const lifecycle = id => ({ id, prefix: '', enabled: true, transitions: [], expiration_days: 30, noncurrent_transitions: [], noncurrent_expiration_days: null, abort_incomplete_days: null });
const cors = () => ({ allowed_origins: ['https://example.test'], allowed_methods: ['GET'], allowed_headers: ['*'], expose_headers: [], max_age_seconds: null });
let serial = 0;

function dependencies(script, key) {
  const mapped = {};
  for (const [, names, , path] of script.matchAll(/import\s*\{([^}]+)\}\s*from\s*(['"])([^'"]+)\2/g)) {
    if (path === '#lib/utils/rule-disclosure.js') { mapped[path] = new URL('../src/lib/utils/rule-disclosure.ts', import.meta.url).href; continue; }
    const exports = names.split(',').map(name => name.trim()).filter(Boolean).map(name => {
      if (name === 'onMount') return `export const onMount=callback=>globalThis.${key}.mounts.push(callback);`;
      if (name === 'onDestroy') return 'export const onDestroy=()=>{};';
      if (name === 'untrack') return 'export const untrack=callback=>callback();';
      if (name === 'appState') return 'export const appState={dateFormat:"iso"};';
      if (name === 'connectionsState') return 'export const connectionsState={s3Profiles:[]};';
      return `export const ${name}=(...args)=>globalThis.${key}.call(${JSON.stringify(name)},args);`;
    });
    mapped[path] = data(exports.join('\n'));
  }
  for (const [, path] of script.matchAll(/from ['"]([^'"]+\.svelte)['"]/g)) mapped[path] = data('export default ()=>{};');
  return mapped;
}
function imports(code, mapped) {
  return code.replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
    const target = mapped[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(target, `Unmapped properties import ${name}`); return `from ${JSON.stringify(target)}`;
  });
}
async function fixture(count) {
  const key = `ruleDisclosures${serial++}`;
  const lists = { lifecycle: Array.from({ length: count }, (_, i) => lifecycle(`rule-${i}`)), cors: Array.from({ length: count }, cors) };
  const context = { mounts: [], calls: [], props: { backend: 's3', path: 's3://bucket/', s3ConnectionId: 'original-connection', capabilities: { lifecycleRules: true, cors: true, storageClasses: [] }, onClose() {} }, call(name, args) {
    context.calls.push([name, ...args]);
    if (name === 's3GetBucketLifecycle') return Promise.resolve(structuredClone(lists.lifecycle));
    if (name === 's3GetBucketCors') return Promise.resolve(structuredClone(lists.cors));
    throw new Error(`Unexpected operation ${name}`);
  } };
  globalThis[key] = context;
  let script = stripTypeScriptTypes(source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1]);
  const mapped = dependencies(script, key);
  script = script.replaceAll('$props()', `globalThis.${key}.props`).replace(/^\s*import \w+ from '[^']+\.svelte';/gm, '');
  script += `\nexport const harness={addLifecycleRule,removeLifecycleRule,toggleLifecycleRule,addCorsRule,removeCorsRule,toggleCorsRule,handleRuleToggleKeydown,toggleCorsMethod,get state(){return{lifecycleRules,corsRules,editingRuleIndex,editingCorsRuleIndex,lifecycleDirty,corsDirty};}};`;
  const code = compileModule(script, { generate: 'client', filename: 'PropertiesController.js' }).js.code;
  const { harness: h } = await import(data(imports(code, mapped)));
  await context.mounts[0](); await Promise.resolve(); await Promise.resolve();
  return { h, ...context, cleanup() { delete globalThis[key]; } };
}

test('shared count/removal policy expands only a sole rule and preserves shifted manual selection', () => {
  for (const count of [0, 1, 2, 10]) assert.equal(initialExpandedRule(count), count === 1 ? 0 : null);
  assert.equal(expandedRuleAfterRemoval(2, 0, 2), 1);
  assert.equal(expandedRuleAfterRemoval(1, 1, 2), null);
  assert.equal(expandedRuleAfterRemoval(null, 0, 1), 0);
  assert.equal(expandedRuleAfterRemoval(0, 0, 0), null);
});

test('actual bucket loading opens single CORS/lifecycle rules and collapses multiple without changing configuration', async () => {
  for (const count of [0, 1, 3]) {
    const f = await fixture(count);
    try {
      assert.equal(f.h.state.editingRuleIndex, initialExpandedRule(count));
      assert.equal(f.h.state.editingCorsRuleIndex, initialExpandedRule(count));
      assert.equal(f.h.state.lifecycleDirty, false); assert.equal(f.h.state.corsDirty, false);
      assert.deepEqual(f.calls.map(call => call[0]).sort(), ['s3GetBucketCors', 's3GetBucketLifecycle']);
      assert.ok(f.calls.every(call => call[1] === 'original-connection'));
      if (count) {
        f.h.toggleLifecycleRule(0); f.h.toggleCorsRule(0);
        assert.equal(f.h.state.editingRuleIndex, count === 1 ? null : 0);
        assert.equal(f.h.state.editingCorsRuleIndex, count === 1 ? null : 0);
        assert.equal(f.h.state.lifecycleDirty, false); assert.equal(f.h.state.corsDirty, false);
        assert.equal(f.calls.length, 2); // Disclosure is not a read or write.
      }
    } finally { f.cleanup(); }
  }
});

test('new rules open explicitly; removing/reindexing rules keeps both editors consistent', async () => {
  const f = await fixture(2);
  try {
    f.h.addLifecycleRule(); f.h.addCorsRule();
    assert.equal(f.h.state.editingRuleIndex, 2); assert.equal(f.h.state.editingCorsRuleIndex, 2);
    f.h.removeLifecycleRule(0); f.h.removeCorsRule(0);
    assert.equal(f.h.state.editingRuleIndex, 1); assert.equal(f.h.state.editingCorsRuleIndex, 1);
    f.h.removeLifecycleRule(1); f.h.removeCorsRule(1);
    assert.equal(f.h.state.editingRuleIndex, 0); assert.equal(f.h.state.editingCorsRuleIndex, 0);
    f.h.toggleLifecycleRule(0); f.h.toggleCorsRule(0);
    assert.equal(f.h.state.editingRuleIndex, null); assert.equal(f.h.state.editingCorsRuleIndex, null);
    f.h.toggleCorsMethod(0, 'PUT');
    assert.equal(f.h.state.editingCorsRuleIndex, null); // Editing never auto-reopens.
    assert.ok(f.h.state.corsRules[0].allowed_methods.includes('PUT'));
    f.h.removeLifecycleRule(0); f.h.removeCorsRule(0);
    assert.equal(f.h.state.editingRuleIndex, null); assert.equal(f.h.state.editingCorsRuleIndex, null);
    assert.equal(f.calls.length, 2); // All mutations remain local until explicit Save.
  } finally { f.cleanup(); }
});

test('rule disclosure buttons keep native keyboard activation without closing the properties dialog', async () => {
  const f = await fixture(1);
  try {
    let stopped = 0;
    for (const key of ['Enter', ' ']) f.h.handleRuleToggleKeydown({ key, stopPropagation() { stopped++; }, preventDefault() { assert.fail('Native button click must remain enabled'); } });
    assert.equal(stopped, 2);
    f.h.handleRuleToggleKeydown({ key: 'Escape', stopPropagation() { assert.fail('Escape still belongs to the dialog'); } });
  } finally { f.cleanup(); }
});

test('actual rule markup shows one body for a sole rule, summaries only for multiple rules, and empty states for zero', async () => {
  const script = stripTypeScriptTypes(source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1]);
  const mapped = dependencies(script, 'unusedSSRRuleFixture');
  mapped.svelte = import.meta.resolve('svelte');
  for (const tab of ['cors', 'lifecycle']) for (const count of [0, 1, 3]) {
    const rules = Array.from({ length: count }, (_, i) => tab === 'cors' ? cors() : lifecycle(`rule-${i}`));
    const seeded = source.replace('let loading = $state(true);', 'let loading = $state(false);')
      .replace('let s3IsBucketRoot = $state(false);', 'let s3IsBucketRoot = $state(true);')
      .replace('let s3IsPrefix = $state(false);', 'let s3IsPrefix = $state(true);')
      .replace(/let bucketTab = [^\n]+;/, `let bucketTab = $state('${tab}');`)
      .replace(new RegExp(`let ${tab}Rules = [^\\n]+;`), `let ${tab}Rules = $state(${JSON.stringify(rules)});`)
      .replace(tab === 'cors' ? 'let editingCorsRuleIndex = $state<number | null>(null);' : 'let editingRuleIndex = $state<number | null>(null);', `let ${tab === 'cors' ? 'editingCorsRuleIndex' : 'editingRuleIndex'} = $state(${JSON.stringify(initialExpandedRule(count))});`);
    const code = compile(seeded, { generate: 'server', filename: 'Properties.svelte' }).js.code;
    const { default: Properties } = await import(data(imports(code, mapped)));
    const html = render(Properties, { props: { backend: 's3', path: 's3://bucket/', s3ConnectionId: 'fixture', capabilities: { cors: true, lifecycleRules: true, storageClasses: [] }, onClose() {} } }).body;
    assert.equal((html.match(/class="lifecycle-rule-body(?:\s|")/g) ?? []).length, count === 1 ? 1 : 0, `${tab} count ${count}`);
    if (count === 1) assert.ok(html.includes('aria-expanded="true"'));
    if (count > 1) assert.ok(html.includes('aria-expanded="false"'));
    if (count === 0) assert.ok(html.includes(tab === 'cors' ? 'No CORS rules' : 'No lifecycle rules'));
  }
});
