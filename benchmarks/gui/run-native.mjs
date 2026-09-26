import { spawn } from 'node:child_process';
import process from 'node:process';

const [binary, entryCount = '100000'] = process.argv.slice(2);
if (!binary) {
  console.error('Usage: node run-native.mjs <binary> [entry-count]');
  process.exit(2);
}

const startedAt = performance.now();
const child = spawn(binary, [], {
  env: { ...process.env, BENCHMARK_ENTRIES: entryCount },
  stdio: ['ignore', 'pipe', 'pipe'],
});

let stdout = '';
let stderr = '';
child.stdout.on('data', (chunk) => { stdout += chunk; });
child.stderr.on('data', (chunk) => { stderr += chunk; });

const timeout = setTimeout(() => child.kill('SIGTERM'), 30_000);
const exitCode = await new Promise((resolve) => child.on('exit', resolve));
clearTimeout(timeout);

if (exitCode !== 0) {
  console.error(stderr || `Native benchmark exited with status ${exitCode}`);
  process.exit(1);
}

const lines = stdout.trim().split('\n');
const result = JSON.parse(lines.at(-1));
result.process_to_result_ms = performance.now() - startedAt;
console.log(JSON.stringify(result));
