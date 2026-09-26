export interface Entry {
  id: number;
  name: string;
  size: number;
  modified: number;
  isDirectory: boolean;
}

export interface BenchmarkResult {
  implementation: string;
  strategy: string;
  entries: number;
  buffer_rows: number;
  generation_ms: number;
  startup_to_first_render_ms: number;
  initial_population_ms: number;
  sort_size_desc_ms: number;
  filter_name_ms: number;
  filtered_entries: number;
  bulk_select_10000_ms: number;
  scroll_200_small_steps_ms: number;
  scroll_200_jumps_ms: number;
  visible_dom_rows: number;
  dom_row_nodes: number;
  used_js_heap_bytes: number | null;
  total_js_heap_bytes: number | null;
}

declare global {
  interface Window {
    __furmanBenchmarkResult?: BenchmarkResult;
  }
}

export const ROW_HEIGHT = 28;
const bufferRowsParam = Number(new URLSearchParams(location.search).get('buffer'));
export const BUFFER_ROWS = Number.isInteger(bufferRowsParam) && bufferRowsParam >= 0
  ? bufferRowsParam
  : 10;
export const LIST_HEIGHT = 630;
export const POOL_SIZE = Math.ceil(LIST_HEIGHT / ROW_HEIGHT) + BUFFER_ROWS * 2;
export const entryCount = Number(new URLSearchParams(location.search).get('entries')) || 100_000;

export function generateEntries(count: number): Entry[] {
  const extensions = ['txt', 'json', 'rs', 'swift', 'png', 'pdf', 'log'];
  return Array.from({ length: count }, (_, index) => {
    const isDirectory = index % 11 === 0;
    const stem = `${isDirectory ? 'folder' : 'report'}-${index.toString().padStart(6, '0')}`;
    return {
      id: index,
      name: isDirectory ? stem : `${stem}.${extensions[index % extensions.length]}`,
      size: (index * 2_654_435_761) % 50_000_000 + 1_024,
      modified: 1_700_000_000 + index % 31_536_000,
      isDirectory,
    };
  });
}

export function memoryUsage() {
  const memory = (performance as Performance & {
    memory?: { usedJSHeapSize: number; totalJSHeapSize: number };
  }).memory;
  return {
    used_js_heap_bytes: memory?.usedJSHeapSize ?? null,
    total_js_heap_bytes: memory?.totalJSHeapSize ?? null,
  };
}

export function publishResult(result: BenchmarkResult) {
  window.__furmanBenchmarkResult = result;
  const output = document.getElementById('benchmark-result');
  if (output) output.textContent = btoa(JSON.stringify(result));
  document.title = 'benchmark-complete';
}
