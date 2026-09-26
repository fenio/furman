# Native GUI Benchmark

This spike compares a native AppKit file list with four browser implementations shaped like Furman's current list mode. Every target generates the same deterministic entries and runs the same operations.

The browser targets are:

- `svelte-current`: keyed sliced rows and a selection set constructed before one reactive assignment, matching Furman's current approach.
- `svelte-optimized`: a fixed pool of reusable Svelte rows and range-based selection.
- `solid`: an idiomatic keyed SolidJS virtual list with range-based selection.
- `vanilla`: an imperative fixed DOM row pool with range-based selection.

## Scenarios

- Generate 100,000 mixed file and directory entries.
- Populate and present the first visible rows.
- Sort all entries by size descending.
- Filter all entries for `report-099`.
- Select the first 10,000 entries.
- Scroll through 200 small three-row steps with heavily overlapping virtual windows.
- Jump through 200 evenly spaced scroll positions.

The AppKit target uses a view-based `NSTableView`. Browser targets default to fixed-height virtualization with ten buffered rows. The pooled targets overwrite a fixed set of row nodes; keyed targets replace rows as the virtual window moves.

## Run

```sh
benchmarks/gui/run.sh
```

The runner requires macOS with `swiftc`, Node dependencies installed, `curl`, and Google Chrome in `/Applications`. The optional arguments are entry count, number of runs, and browser buffer rows on each side of the viewport:

```sh
benchmarks/gui/run.sh 100000 5 10
```

Each run prints one JSON object per implementation, writes the raw objects to `benchmarks/gui/.build/results.ndjson`, and prints a median table. Use several runs; the first run includes cold framework and cache effects.

Set `BENCHMARK_WEB_TARGETS` to a comma-separated subset when isolating one browser implementation:

```sh
BENCHMARK_WEB_TARGETS=svelte-current benchmarks/gui/run.sh 100000 5 4
```

## Metrics

- `startup_to_first_render_ms`: process or navigation start through first populated presentation.
- `process_to_result_ms`: external wall time from launching the process through all scenarios and result collection.
- `initial_population_ms`: assigning all entries through first layout/presentation.
- `sort_size_desc_ms`: sorting, replacing the data source, and layout.
- `filter_name_ms`: filtering, replacing the data source, and layout.
- `bulk_select_10000_ms`: selecting 10,000 rows and layout.
- `scroll_200_small_steps_ms`: 200 three-row scroll steps and layouts, approximating continuous navigation.
- `scroll_200_jumps_ms`: 200 programmatic scroll positions and layout.
- `resident_memory_bytes`: native process resident memory after all scenarios.
- `used_js_heap_bytes`: JavaScript heap after all scenarios.
- `chrome_process_tree_rss_bytes`: aggregate RSS of the headless Chrome browser and its child processes.
- `chrome_benchmark_delta_rss_bytes`: process-tree RSS after the benchmark minus the same Chrome instance on `about:blank`.
- `dom_row_nodes`: row elements retained after the last scroll jump.

## Interpretation Limits

This isolates file-list UI behavior; it does not measure Rust IPC, filesystem enumeration, icons, thumbnails, Git status, drag/drop, or network operations. Chrome's benchmark RSS delta is more useful than its total process-tree RSS, but allocator growth and browser services still make it directional rather than directly equivalent to native resident memory. WKWebView may also share processes with other applications. Neither startup metric predicts packaged application startup: one launches a bare AppKit executable and the other launches a fresh Chrome process rather than Furman's Tauri and WKWebView stack.

The layout timings force synchronous layout and row creation but do not measure end-to-end presentation latency on the display. Current Svelte constructs a normal set and publishes it once, matching Furman's production selection methods. The other browser targets use a range boundary, so the selection metric still compares different representations rather than framework overhead alone.

The benchmark intentionally uses AppKit rather than SwiftUI for the list because `NSTableView` is the proposed native implementation for Furman's high-volume file panes.
