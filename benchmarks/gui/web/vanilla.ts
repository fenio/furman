import {
  BUFFER_ROWS,
  POOL_SIZE,
  ROW_HEIGHT,
  entryCount,
  generateEntries,
  memoryUsage,
  publishResult,
  type Entry,
} from './shared';

export function mountVanilla(target: HTMLElement) {
  target.innerHTML = `
    <main>
      <header><strong>Furman Imperative DOM Benchmark</strong><span id="entry-count">0 entries</span></header>
      <div class="table-header"><span>Name</span><span>Size</span><span>Modified</span></div>
      <div class="list"><div class="spacer"><div class="visible"></div></div></div>
      <pre id="benchmark-result"></pre>
    </main>
  `;

  const countLabel = target.querySelector<HTMLSpanElement>('#entry-count')!;
  const listContainer = target.querySelector<HTMLDivElement>('.list')!;
  const spacer = target.querySelector<HTMLDivElement>('.spacer')!;
  const visible = target.querySelector<HTMLDivElement>('.visible')!;
  const rows = Array.from({ length: POOL_SIZE }, (_, slot) => {
    const row = document.createElement('div');
    row.className = 'row pooled hidden';
    row.style.transform = `translateY(${slot * ROW_HEIGHT}px)`;
    row.append(document.createElement('span'), document.createElement('span'), document.createElement('span'));
    visible.append(row);
    return row;
  });

  let entries: Entry[] = [];
  let selectedCount = 0;

  function renderRows() {
    const startIndex = Math.max(0, Math.floor(listContainer.scrollTop / ROW_HEIGHT) - BUFFER_ROWS);
    visible.style.transform = `translateY(${startIndex * ROW_HEIGHT}px)`;
    for (let slot = 0; slot < rows.length; slot++) {
      const row = rows[slot];
      const entry = entries[startIndex + slot];
      row.classList.toggle('hidden', !entry);
      row.classList.toggle('selected', Boolean(entry && entry.id < selectedCount));
      if (!entry) continue;
      const cells = row.children;
      cells[0].textContent = entry.name;
      cells[1].textContent = entry.isDirectory ? '--' : entry.size.toLocaleString();
      cells[2].textContent = String(entry.modified);
    }
  }

  function setEntries(nextEntries: Entry[]) {
    entries = nextEntries;
    countLabel.textContent = `${entries.length.toLocaleString()} entries`;
    spacer.style.height = `${entries.length * ROW_HEIGHT}px`;
    renderRows();
  }

  function settleLayout() {
    listContainer.getBoundingClientRect();
  }

  listContainer.addEventListener('scroll', renderRows);

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
  const filteredCount = entries.length;

  setEntries(baseline);
  settleLayout();

  const selectionStart = performance.now();
  selectedCount = Math.min(10_000, baseline.length);
  renderRows();
  settleLayout();
  const bulkSelectionMs = performance.now() - selectionStart;

  let smallStepScrollMs = 0;
  if (baseline.length > 1) {
    const maxScroll = Math.max(0, baseline.length * ROW_HEIGHT - listContainer.clientHeight);
    listContainer.scrollTop = 0;
    renderRows();
    settleLayout();
    const smallStepScrollStart = performance.now();
    for (let step = 1; step <= 200; step++) {
      listContainer.scrollTop = Math.min(maxScroll, step * ROW_HEIGHT * 3);
      renderRows();
      settleLayout();
    }
    smallStepScrollMs = performance.now() - smallStepScrollStart;
  }

  const scrollStart = performance.now();
  if (baseline.length > 1) {
    const maxScroll = Math.max(0, baseline.length * ROW_HEIGHT - listContainer.clientHeight);
    for (let step = 0; step < 200; step++) {
      listContainer.scrollTop = step * maxScroll / 199;
      renderRows();
      settleLayout();
    }
  }
  const scrollMs = performance.now() - scrollStart;
  const startIndex = Math.max(0, Math.floor(listContainer.scrollTop / ROW_HEIGHT) - BUFFER_ROWS);

  publishResult({
    implementation: 'vanilla',
    strategy: 'imperative fixed row pool and range selection',
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
    visible_dom_rows: Math.min(POOL_SIZE, Math.max(0, entries.length - startIndex)),
    dom_row_nodes: rows.length,
    ...memoryUsage(),
  });
}
