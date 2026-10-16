import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { compile, compileModule } from 'svelte/compiler';
import { render } from 'svelte/server';
import { detectLanguage, highlightCode, highlightCodeLines, syntaxLanguages } from '../src/lib/utils/highlight.ts';

const data = code => `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const source = readFileSync(new URL('../src/lib/components/Viewer.svelte', import.meta.url), 'utf8');
const highlight = new URL('../src/lib/utils/highlight.ts', import.meta.url).href;
const textOf = html => html.replace(/<\/?(?:span|mark)\b[^>]*>/g, '').replace(/&(amp|lt|gt|quot|#x27);/g, (_, entity) => ({ amp: '&', lt: '<', gt: '>', quot: '"', '#x27': "'" })[entity]);

function imports(code, dependencies) {
  return code.replace(/from (['"])([^'"]+)\1/g, (_, _quote, name) => {
    const target = dependencies[name] ?? (name.startsWith('svelte') ? import.meta.resolve(name) : null);
    assert.ok(target, `Unmapped Viewer import ${name}`); return `from ${JSON.stringify(target)}`;
  });
}

test('Terraform states and backups are JSON, while intentional text/log files are not guessed as code', () => {
  for (const file of ['prod.tfstate', '/private/temp/prod.tfstate.backup', 'PROD.TFSTATE', 'prod.tfstate.lock.info', 'state.tfstate.json']) assert.equal(detectLanguage(file), 'json');
  for (const file of ['notes.txt', 'output.log', 'data.csv']) assert.equal(detectLanguage(file), 'plaintext');
  assert.equal(detectLanguage('Dockerfile'), 'dockerfile');
  assert.equal(detectLanguage('package.json'), 'json');
  assert.equal(detectLanguage('unknown.extension'), undefined);
  assert.equal(detectLanguage('unknown.constructor'), undefined);
  assert.equal(detectLanguage('unknown.__proto__'), undefined);
  assert.ok(syntaxLanguages.some(language => language.id === 'json'));
  assert.ok(syntaxLanguages.some(language => language.id === 'plaintext'));
});

test('JSON distinguishes keys, string values, numbers, boolean/null literals and punctuation without reformatting', () => {
  const code = '{\n  "version": 4, "name": "example", "yes": true, "nothing": null\n}\n';
  const html = highlightCode(code, detectLanguage('example.tfstate'));
  for (const token of ['attr', 'string', 'number', 'literal', 'punctuation']) assert.ok(html.includes(`hljs-${token}`), token);
  assert.match(html, /class="hljs-attr">&quot;name&quot;/);
  assert.match(html, /class="hljs-string">&quot;example&quot;/);
  assert.equal(textOf(html), code);
});

test('numbered multiline comments and strings keep balanced syntax spans on every line', () => {
  const code = '/* first\nsecond */\nconst text = `first\nsecond ${1}`;\n';
  const lines = highlightCodeLines(code, 'javascript');
  assert.equal(lines.length, code.split('\n').length);
  assert.ok(lines[0].includes('hljs-comment')); assert.ok(lines[1].includes('hljs-comment'));
  assert.ok(lines[2].includes('hljs-string')); assert.ok(lines[3].includes('hljs-string'));
  for (const line of lines) {
    let depth = 0;
    for (const match of line.matchAll(/<\/?span\b[^>]*>/g)) {
      depth += match[0].startsWith('</') ? -1 : 1; assert.ok(depth >= 0);
    }
    assert.equal(depth, 0);
  }
  assert.equal(lines.map(textOf).join('\n'), code);
});

test('source markup remains escaped in automatic, explicit and plain-text highlighting', () => {
  const code = '<script>alert("not executable")</script> & <img src=x onerror=alert(1)>';
  for (const language of [undefined, 'json', 'xml', 'plaintext', 'not-a-registered-grammar']) {
    const html = highlightCode(code, language);
    assert.ok(!html.includes('<script')); assert.ok(!html.includes('<img'));
    assert.equal(textOf(html), code);
  }
  assert.equal(highlightCode(code, 'plaintext').includes('hljs-'), false);
});

let serial = 0;
async function viewerFixture() {
  const key = `viewerSyntax${serial++}`;
  const context = { calls: [], mounts: [], props: { path: '/private/fixture.tfstate', mode: 'text', onClose() { context.calls.push('close'); } } };
  globalThis[key] = context;
  const dependencies = {
    svelte: data(`export const onMount=callback=>globalThis.${key}.mounts.push(callback);export const tick=async()=>{};`),
    '#lib/services/tauri.js': data(`export const readFileText=async()=>{globalThis.${key}.calls.push('read');return '{"name":"value"}';};export const readFileBinary=async()=>{throw new Error('Unexpected binary read');};`),
    '@tauri-apps/api/core': data('export const convertFileSrc=path=>path;'),
    '#lib/utils/highlight.js': highlight,
  };
  let script = source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1]
    .replaceAll('$props()', `globalThis.${key}.props`).replace(/^\s*import \w+ from '\.\/[^']+\.svelte';/gm, '')
    // DOM focus/scroll effects are outside this headless controller fixture.
    .replaceAll('$effect(', 'fixtureEffect(');
  script += '\nfunction fixtureEffect() {}';
  script += '\nexport const harness={handleKeydown,markMatches,setSyntax(value){syntax=value;},get state(){return{content,highlightedLines,detectedLanguage,syntax};}};';
  const code = compileModule(stripTypeScriptTypes(script), { generate: 'client', filename: 'Viewer.js' }).js.code;
  const { harness: h } = await import(data(imports(code, dependencies)));
  return { h, ...context, cleanup() { delete globalThis[key]; } };
}

test('actual Viewer syntax override only recolours loaded text: no extra reads, writes or content mutation', async () => {
  const f = await viewerFixture();
  try {
    await f.mounts[0]();
    assert.deepEqual(f.calls, ['read']); assert.equal(f.h.state.detectedLanguage, 'json');
    assert.ok(f.h.state.highlightedLines[0].includes('hljs-attr'));
    const original = f.h.state.content;
    f.h.setSyntax('plaintext'); assert.ok(!f.h.state.highlightedLines[0].includes('hljs-'));
    f.h.setSyntax('json'); assert.ok(f.h.state.highlightedLines[0].includes('hljs-attr'));
    assert.equal(f.h.state.content, original); assert.deepEqual(f.calls, ['read']);
    for (const key of ['q', 'j', 'ArrowDown', ' ']) f.h.handleKeydown({ key, target: { closest: selector => selector === 'select' }, preventDefault() { assert.fail('Native syntax-picker keys must not be intercepted'); } });
    assert.deepEqual(f.calls, ['read']);
    const line = f.h.state.highlightedLines[0]; const marked = f.h.markMatches(line, original, 'value');
    assert.ok(marked.includes('viewer-search-match')); assert.equal(textOf(marked), original);
  } finally { f.cleanup(); }
});

test('Viewer exposes detected JSON, manual syntax and plain text only for text mode', async () => {
  const dependencies = {
    '#lib/utils/highlight.js': highlight,
    './ErrorNotice.svelte': data('export default ()=>{};'),
    '#lib/services/tauri.js': data('export const readFileText=async()=>{throw new Error("No SSR reads");};export const readFileBinary=readFileText;'),
    '@tauri-apps/api/core': data('export const convertFileSrc=path=>path;'),
  };
  const code = compile(source, { generate: 'server', filename: 'Viewer.svelte' }).js.code;
  const { default: Viewer } = await import(data(imports(code, dependencies)));
  const html = mode => render(Viewer, { props: { path: '/private/example.tfstate', mode, onClose() {} } }).body;
  assert.ok(html('text').includes('Auto (JSON)')); assert.ok(html('text').includes('Plain text'));
  assert.ok(!html('hex').includes('Syntax highlighting language'));
  assert.ok(!html('image').includes('Syntax highlighting language'));
});

test('dark and light palettes distinguish all JSON token roles', () => {
  const css = readFileSync(new URL('../src/app.css', import.meta.url), 'utf8');
  const palettes = [...css.matchAll(/(?:\[data-theme="light"\] )?\.hljs \{([^}]*--syntax-key[^}]*)\}/g)];
  const backgrounds = [...css.matchAll(/--bg-primary: (#[\da-f]{6})/g)].slice(0, 2).map(match => match[1]);
  const luminance = hex => {
    const channels = hex.slice(1).match(/../g).map(channel => parseInt(channel, 16) / 255)
      .map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  assert.equal(palettes.length, 2);
  for (const [index, [, palette]] of palettes.entries()) {
    const colours = [...palette.matchAll(/--syntax-(?:key|string|number|literal|punctuation): (#[\da-f]{6})/g)].map(match => match[1]);
    assert.equal(new Set(colours).size, 5);
    for (const colour of colours) {
      const [light, dark] = [luminance(colour), luminance(backgrounds[index])].sort((a, b) => b - a);
      assert.ok((light + 0.05) / (dark + 0.05) >= 4.5, `${colour} must remain readable on ${backgrounds[index]}`);
    }
  }
});
