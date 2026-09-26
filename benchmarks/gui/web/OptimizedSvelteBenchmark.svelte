<script lang="ts">
  import { flushSync, onMount } from 'svelte';
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

  const slots = Array.from({ length: POOL_SIZE }, (_, index) => index);
  let entries = $state<Entry[]>([]);
  let listContainer: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let selectedCount = $state(0);

  const totalHeight = $derived(entries.length * ROW_HEIGHT);
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - BUFFER_ROWS));
  const visibleCount = $derived(Math.min(
    POOL_SIZE,
    Math.max(0, entries.length - startIndex),
  ));
  const offsetY = $derived(startIndex * ROW_HEIGHT);

  function settleLayout() {
    flushSync();
    listContainer?.getBoundingClientRect();
  }

  onMount(() => {
    const generationStart = performance.now();
    const baseline = generateEntries(entryCount);
    const generationMs = performance.now() - generationStart;

    const populationStart = performance.now();
    entries = baseline;
    settleLayout();
    const initialPopulationMs = performance.now() - populationStart;
    const startupToFirstRenderMs = performance.now();

    const sortStart = performance.now();
    entries = [...baseline].sort((a, b) => b.size - a.size);
    settleLayout();
    const sortMs = performance.now() - sortStart;

    const filterStart = performance.now();
    entries = baseline.filter((entry) => entry.name.toLowerCase().includes('report-099'));
    settleLayout();
    const filterMs = performance.now() - filterStart;
    const filteredCount = entries.length;

    entries = baseline;
    settleLayout();

    const selectionStart = performance.now();
    selectedCount = Math.min(10_000, baseline.length);
    settleLayout();
    const bulkSelectionMs = performance.now() - selectionStart;

    let smallStepScrollMs = 0;
    if (baseline.length > 1 && listContainer) {
      const maxScroll = Math.max(0, totalHeight - listContainer.clientHeight);
      listContainer.scrollTop = 0;
      scrollTop = 0;
      settleLayout();
      const smallStepScrollStart = performance.now();
      for (let step = 1; step <= 200; step++) {
        listContainer.scrollTop = Math.min(maxScroll, step * ROW_HEIGHT * 3);
        scrollTop = listContainer.scrollTop;
        settleLayout();
      }
      smallStepScrollMs = performance.now() - smallStepScrollStart;
    }

    const scrollStart = performance.now();
    if (baseline.length > 1 && listContainer) {
      const maxScroll = Math.max(0, totalHeight - listContainer.clientHeight);
      for (let step = 0; step < 200; step++) {
        listContainer.scrollTop = step * maxScroll / 199;
        scrollTop = listContainer.scrollTop;
        settleLayout();
      }
    }
    const scrollMs = performance.now() - scrollStart;

    publishResult({
      implementation: 'svelte-optimized',
      strategy: 'fixed row pool and range selection',
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
      visible_dom_rows: visibleCount,
      dom_row_nodes: listContainer?.querySelectorAll('.row').length ?? 0,
      ...memoryUsage(),
    });
  });

  function handleScroll(event: Event) {
    scrollTop = (event.currentTarget as HTMLDivElement).scrollTop;
  }
</script>

<main>
  <header>
    <strong>Furman Optimized Svelte Benchmark</strong>
    <span>{entries.length.toLocaleString()} entries</span>
  </header>
  <div class="table-header">
    <span>Name</span><span>Size</span><span>Modified</span>
  </div>
  <div class="list" bind:this={listContainer} onscroll={handleScroll}>
    <div class="spacer" style:height={`${totalHeight}px`}>
      <div class="visible" style:transform={`translateY(${offsetY}px)`}>
        {#each slots as slot (slot)}
          {@const entry = entries[startIndex + slot]}
          <div
            class="row pooled"
            class:hidden={!entry}
            class:selected={entry ? entry.id < selectedCount : false}
            style:transform={`translateY(${slot * ROW_HEIGHT}px)`}
          >
            <span>{entry?.name ?? ''}</span>
            <span>{entry ? (entry.isDirectory ? '--' : entry.size.toLocaleString()) : ''}</span>
            <span>{entry?.modified ?? ''}</span>
          </div>
        {/each}
      </div>
    </div>
  </div>
  <pre id="benchmark-result"></pre>
</main>
