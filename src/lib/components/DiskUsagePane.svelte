<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { analyzeDiskUsage, cancelDiskUsage } from '$lib/services/tauri';
  import { formatSize } from '$lib/utils/format';
  import type { DiskUsageEntry, DiskUsageEvent, DiskUsageLevelData } from '$lib/types';
  import {
    Chart, ArcElement, BarElement, CategoryScale, LinearScale,
    DoughnutController, BarController, Tooltip, Legend,
  } from 'chart.js';

  Chart.register(
    ArcElement, BarElement, CategoryScale, LinearScale,
    DoughnutController, BarController, Tooltip, Legend,
  );

  interface Props {
    path: string;
    title: string;
    syncPath?: string;
    onDrillDown?: (path: string) => void;
    onClose: () => void;
  }

  let { path, title, syncPath, onDrillDown, onClose }: Props = $props();

  interface CachedScan {
    entries: DiskUsageEntry[];
    totalSize: number;
    totalFiles: number;
    totalDirs: number;
  }

  let entries = $state<DiskUsageEntry[]>([]);
  let scanning = $state(false);
  let filesScanned = $state(0);
  let totalSize = $state(0);
  let totalFiles = $state(0);
  let totalDirs = $state(0);
  let cancelled = $state(false);
  let scanId = $state('');
  let activeTab = $state<'overview' | 'details'>('overview');

  // Cache of completed scans keyed by path
  const cache = new SvelteMap<string, CachedScan>();

  // Drill-down navigation
  let currentPath = $state('');
  let pathHistory = $state<{ path: string; title: string }[]>([]);
  let currentTitle = $state('');

  // Chart refs
  let doughnutCanvas: HTMLCanvasElement | undefined = $state(undefined);
  let barCanvas: HTMLCanvasElement | undefined = $state(undefined);
  let doughnutChart: Chart | null = null;
  let barChart: Chart | null = null;

  let mounted = false;

  // Background cache prewarming
  let prewarmScanIds: string[] = [];
  let prewarmQueue: DiskUsageEntry[] = [];
  let activePrewarms = 0;
  const MAX_PREWARMING = 3;

  const sortedEntries = $derived(
    [...entries].sort((a, b) => b.size - a.size)
  );

  // Sync from sibling FilePanel: when it navigates, rescan here
  $effect(() => {
    const sp = syncPath;
    if (!mounted || !sp || sp === currentPath) return;
    currentPath = sp;
    currentTitle = sp.replace(/\/+$/, '').split('/').pop() || sp;
    pathHistory = [];
    startScan(sp);
  });

  onMount(() => {
    currentPath = path;
    currentTitle = title;
    mounted = true;
    startScan(path);
  });

  onDestroy(() => {
    if (scanId) cancelDiskUsage(scanId).catch(() => {});
    for (const id of prewarmScanIds) cancelDiskUsage(id).catch(() => {});
    destroyCharts();
  });

  function destroyCharts() {
    doughnutChart?.destroy();
    doughnutChart = null;
    barChart?.destroy();
    barChart = null;
  }

  function restoreFromCache(cached: CachedScan) {
    entries = cached.entries;
    totalSize = cached.totalSize;
    totalFiles = cached.totalFiles;
    totalDirs = cached.totalDirs;
    scanning = false;
    cancelled = false;
    scanId = '';
    filesScanned = 0;
    destroyCharts();
    if (activeTab === 'overview') {
      tick().then(() => buildCharts());
    }
    schedulePrewarm(cached.entries);
  }

  function startScan(scanPath: string) {
    // Check cache first
    const cached = cache.get(scanPath);
    if (cached) {
      restoreFromCache(cached);
      return;
    }

    entries = [];
    scanning = true;
    filesScanned = 0;
    totalSize = 0;
    totalFiles = 0;
    totalDirs = 0;
    cancelled = false;
    prewarmQueue = [];
    destroyCharts();

    const id = 'du-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
    scanId = id;

    analyzeDiskUsage(id, scanPath, (event: DiskUsageEvent) => {
      switch (event.type) {
        case 'Progress':
          filesScanned = event.files_scanned;
          break;
        case 'Entry':
          entries = [...entries, {
            name: event.name,
            path: event.path,
            size: event.size,
            is_dir: event.is_dir,
            item_count: event.item_count,
          }];
          break;
        case 'Level':
          // Cache subdirectory contents emitted during the main scan — free, no extra I/O
          if (!cache.has(event.parent_path)) {
            cache.set(event.parent_path, {
              entries: event.entries,
              totalSize: event.total_size,
              totalFiles: event.total_files,
              totalDirs: event.total_dirs,
            });
          }
          break;
        case 'Done':
          totalSize = event.total_size;
          totalFiles = event.total_files;
          totalDirs = event.total_dirs;
          cancelled = event.cancelled;
          scanning = false;
          scanId = '';
          // Cache completed (non-cancelled) scans
          if (!event.cancelled) {
            const snapshot = [...entries];
            cache.set(scanPath, {
              entries: snapshot,
              totalSize: event.total_size,
              totalFiles: event.total_files,
              totalDirs: event.total_dirs,
            });
            schedulePrewarm(snapshot);
          }
          if (activeTab === 'overview') {
            tick().then(() => buildCharts());
          }
          break;
      }
    }).catch(() => {
      scanning = false;
      scanId = '';
    });
  }

  function handleCancel() {
    if (scanId) cancelDiskUsage(scanId).catch(() => {});
  }

  function prewarmNext() {
    while (activePrewarms < MAX_PREWARMING && prewarmQueue.length > 0) {
      const entry = prewarmQueue.shift()!;
      if (cache.has(entry.path)) continue;

      activePrewarms++;
      const id = 'du-pw-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6);
      prewarmScanIds.push(id);
      let pwEntries: DiskUsageEntry[] = [];

      analyzeDiskUsage(id, entry.path, (event: DiskUsageEvent) => {
        if (event.type === 'Entry') {
          pwEntries.push({ name: event.name, path: event.path, size: event.size, is_dir: event.is_dir, item_count: event.item_count });
        } else if (event.type === 'Done') {
          activePrewarms--;
          prewarmScanIds = prewarmScanIds.filter(i => i !== id);
          if (!event.cancelled) {
            cache.set(entry.path, { entries: pwEntries, totalSize: event.total_size, totalFiles: event.total_files, totalDirs: event.total_dirs });
          }
          prewarmNext();
        }
      }).catch(() => {
        activePrewarms--;
        prewarmScanIds = prewarmScanIds.filter(i => i !== id);
        prewarmNext();
      });
    }
  }

  function schedulePrewarm(completedEntries: DiskUsageEntry[]) {
    // Queue top 10 uncached directory children by size
    const dirs = completedEntries
      .filter(e => e.is_dir && !cache.has(e.path))
      .sort((a, b) => b.size - a.size)
      .slice(0, 10);
    prewarmQueue.push(...dirs);
    prewarmNext();
  }

  function drillDown(entry: DiskUsageEntry) {
    if (!entry.is_dir) return;
    pathHistory = [...pathHistory, { path: currentPath, title: currentTitle }];
    currentPath = entry.path;
    currentTitle = entry.name;
    onDrillDown?.(entry.path);
    startScan(entry.path);
  }

  function navigateTo(index: number) {
    if (index < 0) return;
    const target = pathHistory[index];
    currentPath = target.path;
    currentTitle = target.title;
    pathHistory = pathHistory.slice(0, index);
    onDrillDown?.(target.path);
    startScan(target.path);
  }

  function switchTab(tab: 'overview' | 'details') {
    activeTab = tab;
    if (tab === 'overview' && !scanning && sortedEntries.length > 0) {
      tick().then(() => buildCharts());
    }
  }

  const PALETTE = [
    '#4e79a7', '#f28e2b', '#e15759', '#76b7b2',
    '#59a14f', '#edc948', '#b07aa1', '#ff9da7',
    '#9c755f', '#bab0ac', '#86bcb6', '#8cd17d',
    '#b6992d', '#499894', '#e15759', '#f1ce63',
  ];

  function getColor(i: number): string {
    return PALETTE[i % PALETTE.length];
  }

  function buildCharts() {
    destroyCharts();
    if (sortedEntries.length === 0) return;

    // Doughnut: top 10 + "Other"
    const top10 = sortedEntries.slice(0, 10);
    const otherSize = sortedEntries.slice(10).reduce((sum, e) => sum + e.size, 0);
    const dLabels = top10.map(e => e.name);
    const dData = top10.map(e => e.size);
    if (otherSize > 0) {
      dLabels.push('Other');
      dData.push(otherSize);
    }
    const dColors = dLabels.map((_, i) => getColor(i));

    if (doughnutCanvas) {
      doughnutChart = new Chart(doughnutCanvas, {
        type: 'doughnut',
        data: {
          labels: dLabels,
          datasets: [{ data: dData, backgroundColor: dColors, borderWidth: 0 }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          onClick: (_e, elements) => {
            if (elements.length === 0) return;
            const idx = elements[0].index;
            if (idx < top10.length && top10[idx].is_dir) {
              drillDown(top10[idx]);
            }
          },
          plugins: {
            legend: { display: false },
            tooltip: {
              callbacks: {
                label: (ctx) => {
                  const val = ctx.raw as number;
                  const pct = totalSize > 0 ? ((val / totalSize) * 100).toFixed(1) : '0';
                  return `${ctx.label}: ${formatSize(val)} (${pct}%)`;
                },
              },
            },
          },
        },
      });
    }

    // Bar chart: top 15
    const top15 = sortedEntries.slice(0, 15);
    const bColors = top15.map((_, i) => getColor(i));

    if (barCanvas) {
      barChart = new Chart(barCanvas, {
        type: 'bar',
        data: {
          labels: top15.map(e => e.name.length > 20 ? e.name.slice(0, 20) + '...' : e.name),
          datasets: [{ data: top15.map(e => e.size), backgroundColor: bColors, borderWidth: 0 }],
        },
        options: {
          indexAxis: 'y',
          responsive: true,
          maintainAspectRatio: false,
          onClick: (_e, elements) => {
            if (elements.length === 0) return;
            const idx = elements[0].index;
            if (idx < top15.length && top15[idx].is_dir) {
              drillDown(top15[idx]);
            }
          },
          plugins: {
            legend: { display: false },
            tooltip: {
              callbacks: {
                label: (ctx) => formatSize(ctx.raw as number),
              },
            },
          },
          scales: {
            x: {
              ticks: { callback: (val) => formatSize(val as number), color: '#999' },
              grid: { color: 'rgba(128,128,128,0.15)' },
            },
            y: {
              ticks: { color: '#ccc' },
              grid: { display: false },
            },
          },
        },
      });
    }
  }

  function pctOf(size: number): string {
    if (totalSize <= 0) return '0';
    return ((size / totalSize) * 100).toFixed(1);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="du-pane" role="region" aria-label="Disk Usage" onkeydown={handleKeydown}>
  <!-- Header -->
  <div class="du-header">
    <span class="du-title" title={currentPath}>Disk Usage — {currentTitle}</span>
    <button class="du-close" onclick={onClose} title="Close (Esc)">&#x2715;</button>
  </div>

  <!-- Breadcrumb trail -->
  {#if pathHistory.length > 0}
    <div class="breadcrumb-trail">
      {#each pathHistory as crumb, i (crumb.path)}
        <button class="breadcrumb-item" onclick={() => navigateTo(i)}>
          {crumb.title}
        </button>
        <span class="breadcrumb-sep">/</span>
      {/each}
      <span class="breadcrumb-current">{currentTitle}</span>
    </div>
  {/if}

  <!-- Scan bar -->
  {#if scanning}
    <div class="scan-bar">
      <span class="spinner"></span>
      <span>Scanning... {filesScanned.toLocaleString()} files</span>
      <button class="btn-cancel" onclick={handleCancel}>Cancel</button>
    </div>
  {/if}

  <!-- Tab bar -->
  <div class="tab-bar">
    <button class="tab-btn" class:active={activeTab === 'overview'} onclick={() => switchTab('overview')}>
      Overview
    </button>
    <button class="tab-btn" class:active={activeTab === 'details'} onclick={() => switchTab('details')}>
      Details
    </button>
  </div>

  <!-- Body -->
  <div class="du-body">
    {#if activeTab === 'overview'}
      <div class="du-summary">
        <span>Total: {formatSize(totalSize)}</span>
        <span>{totalFiles.toLocaleString()} files</span>
        <span>{totalDirs.toLocaleString()} dirs</span>
        <span>{sortedEntries.length} items</span>
        {#if cancelled}<span class="du-cancelled">Cancelled</span>{/if}
      </div>

      {#if sortedEntries.length > 0 && !scanning}
        <div class="charts-grid">
          <div class="chart-container">
            <div class="chart-label">Size Distribution (click to drill down)</div>
            <canvas bind:this={doughnutCanvas}></canvas>
          </div>
          <div class="chart-container">
            <div class="chart-label">Top Items by Size</div>
            <canvas bind:this={barCanvas}></canvas>
          </div>
        </div>

        <!-- Legend -->
        <div class="du-legend">
          {#each sortedEntries.slice(0, 10) as entry, i (entry.path)}
            <button
              class="legend-item"
              class:clickable={entry.is_dir}
              onclick={() => entry.is_dir && drillDown(entry)}
            >
              <span class="legend-swatch" style="background: {getColor(i)}"></span>
              <span class="legend-label" title={entry.name}>
                {entry.name.length > 25 ? entry.name.slice(0, 25) + '...' : entry.name}
                {#if entry.is_dir}<span class="legend-dir-icon">&#x25B6;</span>{/if}
              </span>
              <span class="legend-size">{formatSize(entry.size)}</span>
              <span class="legend-pct">{pctOf(entry.size)}%</span>
            </button>
          {/each}
          {#if sortedEntries.length > 10}
            {@const otherSize = sortedEntries.slice(10).reduce((s, e) => s + e.size, 0)}
            <div class="legend-item">
              <span class="legend-swatch" style="background: {getColor(10)}"></span>
              <span class="legend-label">Other ({sortedEntries.length - 10} items)</span>
              <span class="legend-size">{formatSize(otherSize)}</span>
              <span class="legend-pct">{pctOf(otherSize)}%</span>
            </div>
          {/if}
        </div>
      {:else if !scanning}
        <div class="du-empty">No items found.</div>
      {/if}

    {:else}
      <!-- Details tab -->
      <div class="du-table-wrapper">
        <table class="du-table">
          <thead>
            <tr>
              <th>Name</th>
              <th class="right">Size</th>
              <th class="right">%</th>
              <th class="right">Items</th>
              <th>Bar</th>
            </tr>
          </thead>
          <tbody>
            {#each sortedEntries as entry (entry.path)}
              {@const pct = totalSize > 0 ? (entry.size / totalSize) * 100 : 0}
              <tr
                class:is-dir={entry.is_dir}
                class:clickable={entry.is_dir}
                onclick={() => entry.is_dir && drillDown(entry)}
              >
                <td class="name-cell" title={entry.path}>
                  <span class="entry-icon">{entry.is_dir ? '\u{1F4C1}' : '\u{1F4C4}'}</span>
                  {entry.name}
                </td>
                <td class="right">{formatSize(entry.size)}</td>
                <td class="right">{pct.toFixed(1)}</td>
                <td class="right">{entry.item_count.toLocaleString()}</td>
                <td class="bar-cell">
                  <div class="mini-bar" style="width: {Math.max(pct, 0.5)}%"></div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>

<style>
  .du-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .du-header {
    display: flex;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    flex-shrink: 0;
  }

  .du-title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .du-close {
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    padding: 2px 6px;
    border-radius: 3px;
    font-family: inherit;
  }

  .du-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .breadcrumb-trail {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 12px;
    font-size: 11px;
    color: var(--text-secondary);
    flex-wrap: wrap;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .breadcrumb-item {
    background: none;
    border: none;
    color: var(--text-accent);
    cursor: pointer;
    font-size: 11px;
    padding: 1px 3px;
    border-radius: 3px;
    font-family: inherit;
  }

  .breadcrumb-item:hover {
    background: var(--bg-hover);
    text-decoration: underline;
  }

  .breadcrumb-sep {
    opacity: 0.4;
  }

  .breadcrumb-current {
    color: var(--text-primary);
    font-weight: 500;
  }

  .scan-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--border-subtle);
    border-top-color: var(--text-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .btn-cancel {
    padding: 2px 8px;
    font-size: 11px;
    background: var(--bg-hover);
    border: 1px solid var(--border-subtle);
    border-radius: 3px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
  }

  .btn-cancel:hover {
    background: var(--bg-primary);
  }

  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border-subtle);
    padding: 0 12px;
    flex-shrink: 0;
  }

  .tab-btn {
    padding: 6px 14px;
    font-size: 12px;
    font-family: inherit;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .tab-btn:hover {
    color: var(--text-primary);
  }

  .tab-btn.active {
    color: var(--text-accent);
    border-bottom-color: var(--text-accent);
  }

  .du-body {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    min-height: 0;
  }

  .du-summary {
    display: flex;
    gap: 12px;
    padding: 4px 0 8px;
    font-size: 12px;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: 8px;
  }

  .du-cancelled {
    color: var(--warning-color);
    font-weight: 500;
  }

  .charts-grid {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-bottom: 12px;
  }

  .chart-container {
    position: relative;
    height: 200px;
  }

  .chart-label {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 4px;
    text-align: center;
  }

  .du-legend {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 2px;
    font-size: 11px;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 4px;
    background: none;
    border: none;
    border-radius: 3px;
    text-align: left;
    font-family: inherit;
    font-size: 11px;
    color: var(--text-primary);
  }

  .legend-item.clickable {
    cursor: pointer;
  }

  .legend-item.clickable:hover {
    background: var(--bg-hover);
  }

  .legend-swatch {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .legend-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .legend-dir-icon {
    font-size: 8px;
    opacity: 0.5;
    margin-left: 2px;
  }

  .legend-size {
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .legend-pct {
    color: var(--text-secondary);
    width: 40px;
    text-align: right;
    flex-shrink: 0;
  }

  .du-empty {
    text-align: center;
    padding: 30px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .du-table-wrapper {
    overflow-y: auto;
  }

  .du-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }

  .du-table th {
    position: sticky;
    top: 0;
    background: var(--bg-secondary);
    padding: 5px 8px;
    font-weight: 500;
    color: var(--text-secondary);
    text-align: left;
    border-bottom: 1px solid var(--border-subtle);
    z-index: 1;
    font-size: 11px;
  }

  .du-table th.right,
  .du-table td.right {
    text-align: right;
  }

  .du-table td {
    padding: 4px 8px;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border-subtle);
  }

  .du-table tr:hover {
    background: var(--bg-hover);
  }

  .du-table tr.clickable {
    cursor: pointer;
  }

  .du-table tr.is-dir .name-cell {
    font-weight: 500;
  }

  .name-cell {
    display: flex;
    align-items: center;
    gap: 5px;
    max-width: 250px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entry-icon {
    flex-shrink: 0;
    font-size: 12px;
  }

  .bar-cell {
    width: 100px;
    min-width: 60px;
  }

  .mini-bar {
    height: 10px;
    background: var(--text-accent);
    border-radius: 2px;
    opacity: 0.7;
    min-width: 2px;
  }
</style>
