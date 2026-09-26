import { execFileSync, spawn } from 'node:child_process';
import process from 'node:process';

const [url, profilePath, debugPort = '9229'] = process.argv.slice(2);
const chrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const startedAt = performance.now();

if (!url || !profilePath) {
  console.error('Usage: node run-web.mjs <url> <profile-path> [debug-port]');
  process.exit(2);
}

const child = spawn(chrome, [
  '--headless=new',
  '--disable-gpu',
  '--no-first-run',
  '--no-default-browser-check',
  `--user-data-dir=${profilePath}`,
  `--remote-debugging-port=${debugPort}`,
  'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });

let stderr = '';
child.stderr.on('data', (chunk) => { stderr += chunk; });

const deadline = Date.now() + 30_000;
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function getPageTarget() {
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${debugPort}/json/list`);
      const targets = await response.json();
      const target = targets.find((candidate) => candidate.type === 'page');
      if (target?.webSocketDebuggerUrl) return target;
    } catch {
      // Chrome has not opened its debugging endpoint yet.
    }
    await delay(50);
  }
  throw new Error('Timed out waiting for Chrome DevTools');
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const pending = new Map();
    let nextId = 1;

    socket.addEventListener('open', () => {
      resolve({
        socket,
        send(method, params = {}) {
          const id = nextId++;
          socket.send(JSON.stringify({ id, method, params }));
          return new Promise((resolveCommand, rejectCommand) => {
            pending.set(id, { resolve: resolveCommand, reject: rejectCommand });
          });
        },
      });
    });
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (!message.id || !pending.has(message.id)) return;
      const promise = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) promise.reject(new Error(message.error.message));
      else promise.resolve(message.result);
    });
    socket.addEventListener('error', reject);
  });
}

function processTreeResidentBytes(rootPid) {
  const rows = execFileSync('ps', ['-axo', 'pid=,ppid=,rss='], { encoding: 'utf8' })
    .trim()
    .split('\n')
    .map((line) => line.trim().split(/\s+/).map(Number));
  const descendants = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, parentPid] of rows) {
      if (descendants.has(parentPid) && !descendants.has(pid)) {
        descendants.add(pid);
        changed = true;
      }
    }
  }
  return rows
    .filter(([pid]) => descendants.has(pid))
    .reduce((total, [, , rssKilobytes]) => total + rssKilobytes * 1_024, 0);
}

try {
  const target = await getPageTarget();
  const client = await connect(target.webSocketDebuggerUrl);
  await client.send('Runtime.enable');
  await client.send('Page.enable');
  await delay(100);
  const baselineRss = processTreeResidentBytes(child.pid);
  await client.send('Page.navigate', { url });

  let result = null;
  while (Date.now() < deadline && !result) {
    const evaluation = await client.send('Runtime.evaluate', {
      expression: 'window.__furmanBenchmarkResult ?? null',
      returnByValue: true,
    });
    result = evaluation.result?.value ?? null;
    if (!result) await delay(50);
  }

  if (!result) throw new Error('Timed out waiting for the web benchmark result');
  const benchmarkRss = processTreeResidentBytes(child.pid);
  result.chrome_process_tree_rss_bytes = benchmarkRss;
  result.chrome_baseline_rss_bytes = baselineRss;
  result.chrome_benchmark_delta_rss_bytes = benchmarkRss - baselineRss;
  result.process_to_result_ms = performance.now() - startedAt;
  console.log(JSON.stringify(result));
  client.socket.close();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  if (stderr) console.error(stderr);
  process.exitCode = 1;
} finally {
  child.kill('SIGTERM');
}
