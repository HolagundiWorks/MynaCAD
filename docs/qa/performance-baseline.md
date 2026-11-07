# Performance baseline (this campaign)

Hardware and toolchain: [environment.md](environment.md).
Build: `cargo run -p aad-cli --release -- perf N` (release profile).

Historical criterion tables: [PERFORMANCE.md](../PERFORMANCE.md) (2026-10-12). **Do not mix Debug and Release.**

## `aad-cli perf` (release, this machine, 2026-10-26)

| Operation | N=1 000 (ms) | N=10 000 (ms) | N=100 000 (ms) |
|---|---:|---:|---:|
| build synthetic drawing | 0.65 | 4.76 | 45.68 |
| display list build | 73.50 (1578 prims) | 76.15 (17778 prims) | 761.91 (197778 prims) |
| first pick (incl. index) | 0.64 | 4.91 | 53.55 |
| pick ×1000 | 0.51 | 0.55 | 1.14 |
| pick after one edit | 0.33 | 1.29 | 5.65 |
| window/crossing ×20 (10%) | 0.08 | 0.52 | 8.45 |
| fence select (3 legs) | 0.25 | 2.41 | 25.27 |
| osnap ×1000 | 0.59 | 0.90 | 1.33 |
| DXF write | 2.87 (0.2 MB) | 27.73 (2.3 MB) | 316.99 (23.6 MB) |
| DXF read | 4.99 | 47.05 | 513.91 |

Raw outputs (`artifacts/perf-1000.txt`, `perf-10000.txt`, `perf-100000.txt`) were deleted in the 2026-10-12 cleanup; regenerate with `aad-cli perf N`.

Notes:

- Single run each; no median/p95 yet (need repetitions for budgets).
- Display-list at 100k (~762 ms) is **higher** than the ~125 ms full rebuild cited in KNOWN_ISSUES for a different path; treat as a separate measurement (`aad-cli perf` display-list stage) until profiled.
- N=500 000: **not run**.

## WinUI startup

| Metric | Result | Method |
|---|---|---|
| Cold start to titled main window | **470 ms** | Start process; poll until `MainWindowTitle` matches `Tren*`; Debug x64 unpackaged |
| Warm start | not measured | |
| Time to canvas usable / 0 entities chrome | not separately timed | Screenshot at ~few seconds shows empty Drawing1 |

Artifact: [artifacts/winui-cold-start.txt](artifacts/winui-cold-start.txt).

## Not measured yet

Pan/zoom frame times, selection latency in WinUI, hatch ops, undo memory, GPU utilization, input-to-response latency, open/save of large DXF/DWG through the shell.
