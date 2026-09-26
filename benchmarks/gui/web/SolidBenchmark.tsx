import { For, createMemo, createSignal, onMount } from 'solid-js';
import { render } from 'solid-js/web';
import {
  BUFFER_ROWS,
  ROW_HEIGHT,
  entryCount,
  generateEntries,
  memoryUsage,
  publishResult,
  type Entry,
} from './shared';

function Benchmark() {
  const [entries, setEntries] = createSignal<Entry[]>([]);
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportHeight, setViewportHeight] = createSignal(630);
  const [selectedCount, setSelectedCount] = createSignal(0);
  let listContainer!: HTMLDivElement;

  const totalHeight = () => entries().length * ROW_HEIGHT;
  const startIndex = createMemo(() => Math.max(
    0,
    Math.floor(scrollTop() / ROW_HEIGHT) - BUFFER_ROWS,
  ));
  const endIndex = createMemo(() => Math.min(
    entries().length,
    Math.ceil((scrollTop() + viewportHeight()) / ROW_HEIGHT) + BUFFER_ROWS,
  ));
  const visibleEntries = createMemo(() => entries().slice(startIndex(), endIndex()));

  function settleLayout() {
    listContainer.getBoundingClientRect();
  }

  onMount(() => {
    setViewportHeight(listContainer.clientHeight);
    // Solid batches lifecycle updates. Measure in a new microtask so every setter
    // commits its DOM work before the following forced layout.
    queueMicrotask(() => {

      const generationStart = performance.now();
      const baseline = generateEntries(entryCount);
      const generationMs = performance.now() - generationStart;

      const populationStart = performance.now();
      setEntries(baseline);
      settleLayout();
      const initialPopulationMs = performance.now() - populationStart;
      const startupToFirstRenderMs = performance.now();

      const sortStart = performance.now();
      setEntries([...baseline].sort((a, b) => b.size - a.size));
      settleLayout();
      const sortMs = performance.now() - sortStart;

      const filterStart = performance.now();
      setEntries(baseline.filter((entry) => entry.name.toLowerCase().includes('report-099')));
      settleLayout();
      const filterMs = performance.now() - filterStart;
      const filteredCount = entries().length;

      setEntries(baseline);
      settleLayout();

      const selectionStart = performance.now();
      setSelectedCount(Math.min(10_000, baseline.length));
      settleLayout();
      const bulkSelectionMs = performance.now() - selectionStart;

      let smallStepScrollMs = 0;
      if (baseline.length > 1) {
        const maxScroll = Math.max(0, totalHeight() - listContainer.clientHeight);
        listContainer.scrollTop = 0;
        setScrollTop(0);
        settleLayout();
        const smallStepScrollStart = performance.now();
        for (let step = 1; step <= 200; step++) {
          listContainer.scrollTop = Math.min(maxScroll, step * ROW_HEIGHT * 3);
          setScrollTop(listContainer.scrollTop);
          settleLayout();
        }
        smallStepScrollMs = performance.now() - smallStepScrollStart;
      }

      const scrollStart = performance.now();
      if (baseline.length > 1) {
        const maxScroll = Math.max(0, totalHeight() - listContainer.clientHeight);
        for (let step = 0; step < 200; step++) {
          listContainer.scrollTop = step * maxScroll / 199;
          setScrollTop(listContainer.scrollTop);
          settleLayout();
        }
      }
      const scrollMs = performance.now() - scrollStart;

      publishResult({
        implementation: 'solid',
        strategy: 'keyed sliced rows and range selection',
        entries: entryCount,
        buffer_rows: BUFFER_ROWS,
        generation_ms: generationMs,
        startup_to_first_render_ms: startupToFirstRenderMs,
        initial_population_ms: initialPopulationMs,
        sort_size_desc_ms: sortMs,
        filter_name_ms: filterMs,
        filtered_entries: filteredCount,
        bulk_select_10000_ms: bulkSelectionMs,
        scroll_200_small_steps_ms: smallStepScrollMs,
        scroll_200_jumps_ms: scrollMs,
        visible_dom_rows: visibleEntries().length,
        dom_row_nodes: listContainer.querySelectorAll('.row').length,
        ...memoryUsage(),
      });
    });
  });

  return (
    <main>
      <header>
        <strong>Furman SolidJS Benchmark</strong>
        <span>{entries().length.toLocaleString()} entries</span>
      </header>
      <div class="table-header">
        <span>Name</span><span>Size</span><span>Modified</span>
      </div>
      <div
        class="list"
        ref={(element) => { listContainer = element; }}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      >
        <div class="spacer" style={{ height: `${totalHeight()}px` }}>
          <div
            class="visible"
            style={{ transform: `translateY(${startIndex() * ROW_HEIGHT}px)` }}
          >
            <For each={visibleEntries()}>{(entry) =>
              <div class="row" classList={{ selected: entry.id < selectedCount() }}>
                <span>{entry.name}</span>
                <span>{entry.isDirectory ? '--' : entry.size.toLocaleString()}</span>
                <span>{entry.modified}</span>
              </div>
            }</For>
          </div>
        </div>
      </div>
      <pre id="benchmark-result"></pre>
    </main>
  );
}

export function mountSolid(target: HTMLElement) {
  render(() => <Benchmark />, target);
}
