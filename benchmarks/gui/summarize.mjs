import { readFileSync } from 'node:fs';
import process from 'node:process';

const [resultsPath] = process.argv.slice(2);
if (!resultsPath) {
  console.error('Usage: node summarize.mjs <results.ndjson>');
  process.exit(2);
}

const results = readFileSync(resultsPath, 'utf8')
  .trim()
  .split('\n')
  .filter(Boolean)
  .map((line) => JSON.parse(line));

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

const metrics = [
  ['initial_population_ms', 'populate'],
  ['sort_size_desc_ms', 'sort'],
  ['filter_name_ms', 'filter'],
  ['bulk_select_10000_ms', 'select'],
  ['scroll_200_small_steps_ms', 'small scroll'],
  ['scroll_200_jumps_ms', 'jump scroll'],
  ['used_js_heap_bytes', 'heap MiB'],
];
const implementations = [...new Set(results.map((result) => result.implementation))];

console.log('\nMedian results');
console.log(['implementation', ...metrics.map(([, label]) => label)].join('\t'));
for (const implementation of implementations) {
  const runs = results.filter((result) => result.implementation === implementation);
  const cells = metrics.map(([key]) => {
    const values = runs.map((run) => run[key]).filter(Number.isFinite);
    if (values.length === 0) return 'n/a';
    const value = median(values);
    return key.endsWith('_bytes') ? (value / 1_048_576).toFixed(1) : value.toFixed(2);
  });
  console.log([implementation, ...cells].join('\t'));
}
