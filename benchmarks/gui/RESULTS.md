# Initial Results

Run on 2026-09-25 using 100,000 entries and five iterations per implementation.

## Environment

- MacBook Air, Apple M5, 10 cores, 24 GB memory
- macOS 27.0 (26A428)
- Swift 6.4, optimized with `swiftc -O`
- Svelte 5.53.6, SolidJS 1.9.15, and Vite 8.3.0 production builds
- Google Chrome 154.0.8037.58, headless

## Five-Run Medians

| Metric | AppKit | Current Svelte | Pooled Svelte | SolidJS | Vanilla DOM |
|---|---:|---:|---:|---:|---:|
| External process to complete result | 319.2 ms | 692.2 ms | 734.6 ms | 657.2 ms | 732.6 ms |
| Generate 100k synthetic entries | 71.2 ms | 10.5 ms | 10.3 ms | 10.1 ms | 10.8 ms |
| Initial population and layout | 13.1 ms | 1.9 ms | 6.3 ms | 1.3 ms | 9.6 ms |
| Sort 100k entries by size | 4.7 ms | 14.7 ms | 14.8 ms | 13.5 ms | 14.2 ms |
| Filter 100k names | 47.2 ms | 5.8 ms | 6.1 ms | 5.7 ms | 5.8 ms |
| Select 10k entries | 0.04 ms | 0.4 ms | 0.2 ms | 0.1 ms | 0.7 ms |
| 200 small scroll steps and layouts | 42.8 ms | 25.7 ms | 113.4 ms | 21.4 ms | 109.5 ms |
| 200 scroll jumps and layouts | 33.0 ms | 196.2 ms | 150.0 ms | 174.1 ms | 138.6 ms |
| Resident memory / JS heap | 99.1 MiB | 36.1 MiB | 26.4 MiB | 19.7 MiB | 11.6 MiB |

## Interpretation

The result does not justify replacing Svelte with SolidJS. Solid is about 17% faster for overlapping small scroll steps and 11% faster for large jumps. That is not enough to justify another UI framework or an application rewrite, especially after applying the overscan reduction described below.

Naive fixed row pooling is not a general improvement. It reduces Svelte's large-jump time by 24%, but it is 4.4x slower when adjacent virtual windows overlap because all 43 row contents are overwritten at every step. Imperative DOM has the same tradeoff: large jumps are 29% faster than current Svelte, while small steps are 4.3x slower. Current keyed reconciliation efficiently preserves overlapping rows.

Furman's production selection methods already construct a set before one reactive assignment. The corrected current-Svelte target selects 10,000 entries in 0.4 ms, so selection representation is not a meaningful production bottleneck. Range-backed alternatives remain faster, but the absolute difference is negligible.

JavaScript remains substantially faster for this synthetic generation and lowercase substring filter. AppKit remains substantially faster for sorting and large random scroll jumps. Small-step layout results are not equivalent end-to-end frame measurements: AppKit's `displayIfNeeded` and Chrome's forced layout have different painting and presentation boundaries. The external process figures are also diagnostic only because they compare a bare executable with a fresh Chrome process.

Memory figures are not directly comparable: AppKit reports process RSS while browser targets report JavaScript heap. Chrome process-tree deltas are retained in the raw NDJSON output but are too noisy and browser-specific to support architecture conclusions.

## Overscan Sweep

Three additional current-Svelte runs at each buffer size measured the cost of rows retained above and below the viewport:

| Buffer rows | Small scroll | Large jumps | Final DOM rows |
|---:|---:|---:|---:|
| 2 | 25.0 ms | 135.6 ms | 25 |
| 4 | 25.8 ms | 149.4 ms | 27 |
| 6 | 27.9 ms | 176.1 ms | 29 |
| 10 | 28.6 ms | 211.8 ms | 33 |

Furman now uses four buffer rows. This keeps more safety margin than the fastest two-row result while reducing large-jump work by about 29% relative to ten rows. Actual blank-frame behavior still needs validation under trackpad momentum in the packaged WebView.

## Recommendation

Keep Svelte and its keyed virtual list. Do not migrate to SolidJS or replace the pane with the naive fixed pool measured here. Furman already batches selection publication; the implemented optimizations instead reduce overscan, avoid deep proxies for immutable entries, accelerate locale-aware sorting, and reduce rubber-band work. Any further renderer experiment should update only dirty pooled rows and must be evaluated with real animation-frame latency, accessibility, keyboard selection, and drag/drop enabled.

## Next Benchmark

The next useful benchmark should replace synthetic generation and forced layout loops with the real data path and interactive frame measurements:

1. Stream identical Rust directory batches into current and pooled Svelte panes.
2. Measure time to first 1,000 rows and completion of 100,000 rows.
3. Record animation-frame latency during real wheel/trackpad scrolling.
4. Exercise keyboard range selection, discontinuous selection, drag/drop, and live watcher updates.
5. Compare the winner with an AppKit pane receiving the same Rust batches.

That will isolate whether the expected native gain survives the real Rust-to-UI data path.
