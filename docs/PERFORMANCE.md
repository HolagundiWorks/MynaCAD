# Performance baseline

Criterion tables below were measured on 2026-10-12. Interactive path and I/O behaviour updates
(2026-10-23) are at the end of this page; re-measure with `aad-cli perf` before quoting new absolute
times. UI frame numbers: [UI_BASELINE.md](UI_BASELINE.md). No hardware targets are set yet.

## How it was measured

- Windows 11, 16 threads, 15.8 GB, rustc 1.99.0, `cargo bench` (release profile, criterion 0.5).
- Each benchmark: 1 s warm-up, 3 s measurement, 100 samples (10 to 20 for the slow ones). Values are
  criterion's `[low estimate high]`; the table shows the estimate.
- One machine, one run, nothing else running. Treat differences under about 10% as noise: the
  first geometry run (a few minutes earlier, while other work was running) and the second differ by
  up to 2x on the intersection benches, and the DXF benches report changes of 7 to 18% between two
  runs. Use these numbers for orders of magnitude and for before/after on the same machine, not as
  absolutes.
- Data: `tools/quality/synthetic.rs`, a deterministic drawing (lines, circles, arcs, bulging
  polylines and text, equal shares, constant density). It is not a real architectural drawing.
- Commands: `cargo bench -p aad-geom --bench geometry`, `-p aad-engine --bench selection`,
  `--bench drawing_operations`, `-p aad-render --bench render`, `-p aad-io --bench dxf`,
  each followed by `-- --warm-up-time 1 --measurement-time 3`. Raw output:
  `target/quality/bench-run2.txt` and `bench-run2-ops.txt`; these raw files were deleted by the 2026-10-12 cleanup (the tables here are the record; rerun the commands to regenerate).

## Geometry (`aad-geom`)

| Operation | Time |
|---|---|
| line-line x 1 000 pairs | 9.7 us |
| line-circle x 1 000 | 12.6 us |
| circle-circle x 1 000 pairs | 16.1 us |
| `Line::closest` x 1 000 | 1.89 us |
| `Mat3::apply` x 1 000 points | 0.70 us |
| point-in-polygon, 100 / 1 000 / 10 000 vertices | 74 ns / 705 ns / 6.9 us |
| shoelace area, 100 / 1 000 / 10 000 vertices | 66 ns / 677 ns / 6.8 us |
| polyline area with bulges, 100 / 1 000 / 10 000 | 1.14 us / 11.6 us / 115 us |
| polyline tessellate, 100 / 1 000 / 10 000 | 9.7 us / 79.7 us / 739 us |
| arc tessellate, r = 1000, tolerance 0.01 | 7.2 us |

All scale linearly with vertex count.

## Selection and snapping (`aad-engine`)

| Operation | 10 000 entities | 100 000 entities |
|---|---|---|
| first pick (builds the index) | 1.74 ms | 5.89 ms |
| pick x 200, index warm | 77 us | 120 us |
| window select, 10% side | 7.9 us | 110 us |
| crossing select, 10% side | 13.8 us | 127 us |
| fence select | 1.70 ms | 21.6 ms |
| object snap x 200 | 93 us | 140 us |

The first pick here is 5.9 ms at 100 000 entities, against the 49.5 ms frame seen in the application
(UI-001). I have not explained the difference; the application frame also contained other work
that was not isolated. Both are one-time costs, which is why the index is now built in
the background. Picking is sub-linear: 12 times the entities cost 1.6 times as much per pick.
Fence selection is the one operation that scales close to linearly (12.7 times for 10 times the
entities).

## Editing 1 000 objects in a 100 000-entity drawing (`Session::execute`)

| Command | Time |
|---|---|
| add one line | 386 us |
| copy ~1 000 | 2.67 ms |
| undo of a move of ~1 000 | 0.62 ms |
| **move ~1 000** | **41.8 ms** |
| **rotate ~1 000** | **40.3 ms** |
| **erase ~1 000** | **39.2 ms** |
| **recolour ~1 000** | **40.3 ms** |

**Finding (hypothesis, not yet profiled):** commands that modify existing objects cost about 40 ms
each at 100 000 entities, 15 times a copy of the same objects and 65 times their undo. The time is
suspiciously flat across four different operations, which points at something they share (for
example copying or touching every chunk of the entity store, or an index or extents recomputation
after the edit) rather than at the geometry. It is below the 100 ms feel threshold but is the
largest single cost of a routine edit. Recorded as PERF-001 in [BUG_BACKLOG.md](BUG_BACKLOG.md); it
needs a profile before any change.

## Display list (`aad-render`)

| Operation | 10 000 | 100 000 |
|---|---|---|
| model space at zoom extents | 13.3 ms | 129 ms |
| layout with one viewport (clipped) | 4.95 ms | 51.9 ms |
| one pan step inside that viewport | 2.72 ms | 51.9 ms |

Linear in entity count. The pan step costs the same as building the layout, because the viewport
content is rebuilt when the view centre moves (since the BUG-002 fix); at 100 000 entities that is
52 ms per pan frame, a candidate for caching.

## DXF read and write (`aad-io`)

| Entities | write | read |
|---|---|---|
| 10 000 | 29.3 ms | 43.3 ms |
| 50 000 | 140 ms | 222 ms |

Linear. The one-shot figures from `aad-cli perf` for 100 000 and 1 000 000 entities are in
[UI_BASELINE.md](UI_BASELINE.md); they agree with these (about 4.4 us per entity to read).

## Not measured

Memory use, cold-start time, DWG read, hatch and dimension rendering at scale, plot and PDF export,
snapping with many active snap modes, and any machine other than this one.

## Reproduce

```text
cargo bench -p aad-engine --bench selection -- --warm-up-time 1 --measurement-time 3
AAD_BENCH_LARGE=1 cargo bench -p aad-engine --bench selection     # adds 1 000 000 entities
```

## Update: PERF-001 fixed

The 40 ms edits were the non-finite-result check, not the edits (profile and fix in
[BUG_BACKLOG.md](BUG_BACKLOG.md)). Same benchmark after the fix, one run:

| Command (about 1 000 objects, 100 000 entities) | Before | After |
|---|---|---|
| move | 41.8 ms | 5.30 ms |
| rotate | 40.3 ms | 5.36 ms |
| erase | 39.2 ms | 5.64 ms |
| recolour | 40.3 ms | 5.61 ms |
| copy | 2.67 ms | 2.43 ms |
| undo of a move | 0.62 ms | 0.48 ms |
| add one line | 386 us | 192 us |

The "finding" paragraph above describes the state before the fix.

## Update: display-list build of text-heavy drawings

Measured with the ignored probes in `apps/aad-cli/tests/perf_probe.rs` (`AAD_PERF_FILE=<drawing>`,
`cargo test --release -p aad-cli --test perf_probe -- --ignored --nocapture --test-threads=1`) on a
drawing with 705 474 primitives, most of it filled TrueType text. Text fill dominated the build.

| Step | Display-list build | Triangle vertices |
|---|---|---|
| Slab trapezoids (`triangulate_evenodd`) | about 1.0 to 1.6 s | 37.3 M |
| Ear clipping (`triangulate_outline`, `earcutr`) | about 0.32 to 0.38 s | 12.1 M |

`triangulate_outline` classifies loops by nesting depth, triangulates each outer boundary with its
holes, and falls back to `triangulate_evenodd` when the result area differs from the outline area or
the input is hostile (too many loops or points). An earlier world-space per-glyph cache was tried
and removed (key quantisation risk). Phase D instead retains **font-unit** triangulations keyed by
`(font, glyph_id, band, fill)` — see the Phase D update below.

Still open as absolute CPU: DWG parse (about 1.2 to 1.5 s in `acadrust` + DXF bridge; set
`AAD_DWG_STAGE=1` for spawn timing) and DXF read/write (about 0.55 s at 100k). Split DXF benches
before micro-optimising.

## Update: interactive pan / edit path (WinUI + layout viewports)

- **WinUI host** (`aad-host`): caches the display list keyed by document revision, tessellation
  band, LOD flags and space; model-space edits use `aad_render::rebuild` when safe; the GPU mesh
  is keyed by revision + snapped origin + sheet tint so fine pans only rewrite the view uniform.
  Crossing a zoom/LOD band keeps the last list on screen and rebuilds on a worker (`PendingList`).
  `spatial::warm` runs when the list is rebuilt. Far zoom turns off text/fill via
  `aad_render::lod_for_view`; model builds can pass `view_center` for frustum cull.
- **Layout viewports** (`aad-render`): model geometry is tessellated in world coordinates into a
  thread-local LRU (6 entries) keyed by content + scale + a coarse pan cell (**8×** view height)
  with a pad that covers the cell. Fine and medium pans only transform and clip; a jump across
  cells uses the LRU; zoom still retessellates (exact height / PSLTSCALE). Regression:
  `crates/render/tests/viewport_pan_cache.rs`, `lod_and_frustum.rs`. WinUI fine pans keep the mesh
  key (`fine_pan_keeps_the_cached_mesh`).

## Update: GPU analytic curves + inspect perf fields (Phase A)

- **Implemented:** continuous ellipses and non-uniform-scaled circles/arcs emit `Kind::Ellipse`
  (conjugate diameters U/V) into the shared ring instance buffer; continuous similarity circles/arcs
  stay `Kind::Arc`. Dashed linetypes and degenerate axes still flatten to polylines. CPU raster and
  clip paths tessellate rings; `aad_render::sweep_radians` expands stored `f32` sweeps so a full
  circle does not collapse when `f32::TAU` exceeds `f64::TAU`.
- **Inspect hooks:** WinUI and egui `ui.inspect` `perf` include `buildMs`, `meshMs`, `prims`,
  `rings`, and `meshBytes` (egui also keeps `frameMs` / `drawMs` / `renderer`). Absolute times are
  not re-measured here — use `aad-cli perf` / `perf_probe` on a reference machine before quoting.
- **Proposed (not in this change):** tiled display-list scene (Phase C), glyph atlas (Phase D),
  Vello spike, WinUI GPU timestamp queries behind a flag.

## Update: spatial mesh tiles + dirty upload (Phase B)

- **Implemented:** `aad_render::mesh` builds `MeshData` for a prim-index slice
  (`build_mesh_prims` / `partition_prims` / `tile_content_fingerprint` / `merge_meshes`).
  WinUI `FrameCache` and egui `CanvasState` retain per-tile meshes keyed by content fingerprint ⊕
  mesh basis (origin / tint / band / lineweight). On revision change, only dirty tiles remesh;
  fine pans still keep the scene key (`fine_pan_keeps_the_cached_mesh`). WinUI
  `Gpu::present_tiles` uploads only tiles whose content key changed and draws them in tile order;
  infinites stay a small per-present extra. Monolithic `build_mesh_mode` remains when tile count
  is below `MIN_MESH_TILES`, above `MAX_MESH_TILES`, or a wipeout spans tiles
  (`mesh_tiling_viable` / `wipeout_spans_tiles`). Regression: `edit_only_remeshes_dirty_tiles`.
- **Not claimed here:** absolute `meshMs` / `meshBytes` before/after on a reference machine
  (re-measure with `aad-cli perf` / `ui.inspect`).

## Update: tiled display-list scene (Phase C)

- **Implemented:** `aad_render::scene` retains a model-space `Scene` of world-grid tiles
  (`SceneTile` = `Arc<DisplayList>`). Entities land by bounds centroid; rays/xlines use
  `GLOBAL_TILE`. `build_scene` / `rebuild_scene` tessellate only dirty tiles (shared
  `MAX_BLOCK_EXPANSION` budget, rayon within/across dirty tiles); `flatten_scene` restores
  document draw order. Frustum builds skip far tiles while `extents` still unions the full
  model. Layout viewport model cache is a process-wide `Mutex` LRU (`vp_model_get` /
  `vp_model_put`), not `thread_local`, so worker threads share it. WinUI `FrameCache` keeps
  an optional `Scene` and prefers `rebuild_scene` on model edits when environment is unchanged;
  plot / environment changes still take full `build`. Hostile caps: `MAX_SCENE_TILES` /
  `scene_tiling_viable` (same band as mesh tiles). Regressions: `scene_tiles`,
  `viewport_pan_cache`, `lod_and_frustum`, `parallel_build`.
- **Not claimed here:** worker extents rebuild median ≤ ~80 ms @ 100k on a reference machine
  (re-measure with `aad-cli perf` / `ui.inspect`).

## Update: text and hatch retention (Phase D)

- **Implemented:** CPU triangulated glyph×band mesh cache in `aad_render::glyph_mesh`, keyed by
  `(font, glyph_id, flatten band, fill)`. TrueType shaping keeps `GlyphMeshKey` + `GlyphPlace`;
  filled display-list builds reuse font-unit triangles and apply place + `Shaped::post_xf`. Cap:
  `MAX_GLYPH_MESH_ENTRIES` (clear-on-overflow). Hatch pattern expansion is retained per
  `(boundary hash, pattern, embedded pattern_lines hash, scale, angle, origin, band)` inside
  `aad_render::hatch` (`MAX_HATCH_PATTERN_ENTRIES`). Both caches are process-wide and content-keyed,
  so Phase C dirty-tile `rebuild_scene` reuses them without extra invalidation; a new tessellation
  band selects different keys. Regressions: `glyph_mesh` unit tests, `text_hatch_retention`.
- **Prototype decision:** ship the CPU glyph×band triangulated cache first. GPU glyph atlas
  (R8 coverage) and Loop–Blinn filled TTF remain **proposed** — pick after a measured prototype on
  a 705k-prim class drawing.
- **Not claimed here:** absolute text-heavy `perf_probe` before/after on a reference machine;
  GPU atlas / Loop–Blinn.

## Update: optional Vello paper/plot PNG spike (Phase E)

- **Implemented (prototype wiring only):** L5 crate `aad-vello` rasterises a `DisplayList` with
  Linebender `vello_cpu` (Apache-2.0 OR MIT). Host / egui / MCP `ui.render` call
  `aad_vello::preview_png`, which uses `vello_cpu` only when `AAD_CANVAS=vello` (case-insensitive);
  unset or any other value keeps `aad_render::raster` (tiny-skia). The interactive mesh/wgpu canvas
  never reads this flag. `aad-render` stays wgpu/Vello-free (L2). Regressions: `aad-vello` lib tests
  (flag parse defaults to custom; custom dispatch equals tiny-skia; vello path emits PNG).
- **Proposed / unmeasured:** sheet quality win vs tiny-skia on nameplates; GPU `vello` for live
  paper-space; replacing any Phase A–D custom path. Evaluate by exporting the same layout with the
  flag off and on, then comparing AA on arcs, text and hatches. **Verdict gate:** keep the spike
  only if measured quality beats the custom path without pan regression or layering breaks.

## Update: file I/O path

- Control `app.open` / `app.save`: start worker, defer the JSON reply until `Host::pump` polls
  completion (client still waits on one reply). Direct `control_one` in tests polls locally.
- WinUI menu Save / Save As use `SaveFileAsync` (`SaveStart`/`SavePoll`), same as Open.
- DWG worker (Windows): Job Object process memory cap ~2 GiB (`aad_dwg::isolated::WORKER_MEMORY_CAP`).

Re-measure large drawings with `aad-cli perf` / `perf_probe` on the same machine before claiming
new absolute times; the structural change removes full rebuilds from the common pan path.

## Interactive audit with a real drawing (2026-10-30)

Test drawing: `DEMO.dwg` (OneDrive\Documents\OSAS TEST, 1.3 MB, 9 672 entities, seven sheets of plans, details and title blocks). Machine: RTX 4060 laptop GPU, 16 GB. All numbers are from the shell's own control port, so they include the whole path (command, display list, mesh, GPU present, panels).

### What was already there

MynaCAD already draws on the GPU through its own engine: `aad-render` builds a display list (LOD by zoom band, view culling, tiled meshes, a worker thread for band rebuilds) and `aad-host/gpu.rs` presents it with wgpu on DirectX 12, `HighPerformance` adapter, vsync (`AAD_VSYNC=0` turns it off). So the slowness was not "no hardware acceleration" and a new graphics engine would not have fixed it. It was CPU work and data volume feeding the GPU, and UI-thread work after every change.

### What was measured, and what was slow

| Step | Before | After |
|---|---|---|
| A zoom command (rpc, UI thread blocked) | 320 to 680 ms | 15 to 50 ms |
| A pan command | 330 to 490 ms | 15 to 30 ms |
| Mesh data at one zoom step (words sent to the GPU) | 14.3 million | 0.33 million |
| Display list at the next steps | 392 000 primitives, 4.7 million triangle vertices | 25 600 primitives, 27 000 triangle vertices |
| Display list build, one zoom step | 90 to 160 ms | 39 ms |
| Mesh build | 32 to 150 ms | 1.5 to 5 ms |
| Open `DEMO.dwg` (parse + first list) | 0.4 s + 0.25 s | unchanged (one-time) |

Causes, found with `AAD_PERF=1` and `cargo run --release -p aad-host --example profile_render -- file.dwg`:

1. **Text and hatches drawn at full detail however small they were on screen.** The LOD switches were absolute (world units per pixel), so they did not suit a drawing in other units; 4.7 million triangle vertices of glyphs and 340 000 hatch lines were built for text and patterns a few pixels tall. Now each text is drawn by its own height on screen (under 1.5 px: not drawn; under 6 px: a grey bar; else glyphs) and each pattern hatch whose line spacing is under 3 px becomes a tint (`Options::screen_px`; `render/src/lib.rs`, `hatch.rs`).
2. **Every zoom command rebuilt the display list on the UI thread.** The `zoom` command bumped the document revision, which the display-list cache treats as an edit. In model space it no longer does (the cache is keyed by zoom band and pan cell); paper space and viewports still redraw.
3. **Every pointer move over a new object rebuilt all side panels (about 1 s).** The Blocks thumbnails (0.5 s), the Layers list, the quick-properties window and the side rail were rebuilt on each hover change. Now only the panel on screen is rebuilt (the others when their title is chosen), the rail restyles only what changed, a hidden Layer Manager is not reloaded, and the quick-properties window is only rebuilt when its content changes.

### Tools left in

- `AAD_PERF=1` (set before starting MynaCAD): ticks of the UI loop over 40 ms, with the time of each step, and host redraws over 20 ms, go to `%LOCALAPPDATA%\OSAS\odraft-perf.log`. Free when off.
- `cargo run --release -p aad-host --example profile_render -- <file.dwg> [cx cy height]`: display list and mesh time and size at six zoom steps, and what text and fill cost.

### Still slow

- Opening a large drawing builds the first display list on the UI thread (about 0.2 s for this one).
- A first zoom that crosses a zoom band still builds a list (39 ms here, on a worker), so a very large drawing will show the old detail for a moment.
- Interactive wheel zoom and middle-button pan were not timed separately (the control port drives commands); they use the same list cache.

## Update: zoom remeshing and first display list (2026-11-05)

Test drawing: a 3.5 MB DWG with 26 206 entities that expand to 710 250 primitives and 15 M triangle
vertices at full detail (text fill, hatch lines, block references). Machine as above. Measured with
`AAD_PERF=1` (see the interactive audit above) on a copy of the installed app with a patched
`aad_host.dll`.

| Step | Before | After |
|---|---|---|
| One wheel-zoom tick, mesh rebuild on the UI thread | 35 to 57 ms (frames 20 to 60 ms) | no frame over 20 ms in the same zoom and pan run |
| Opening the drawing, UI blocked by the first display list | about 400 to 460 ms | not on the UI thread |

- **Zoom:** the mesh origin step and the tile edge came from the exact view height, so every wheel tick
  changed the origin and every tile key and remeshed the whole drawing. `aad_render::mesh::quantize_view_height`
  now rounds the height down to a power of two for both (`choose_origin`, `mesh_tile_edge`); ticks inside one
  octave keep the cached meshes. The origin-to-view offset stays under 32 view heights, so `f32` precision is
  unchanged. Regression: `zoom_ticks_inside_one_octave_keep_origin_and_tile_edge`.
- **Open:** for a windowed host and a drawing of 3 000 or more entities (`ASYNC_FIRST_BUILD_ENTITIES`) the first
  list is built on the `aad-first-list` worker with the spatial index warm-up; `Host::mesh` returns a "loading"
  state, `redraw` skips the frame and `pump` keeps polling. Edits, band changes, tests and small drawings keep the
  synchronous paths.
- **Not a cause:** building the tile meshes in parallel changed nothing (the tile phase is 8 to 40 ms), so it was
  not kept. The host's "mesh" figure in the perf log includes the synchronous list build, which is why it first
  looked like a mesh cost.
- **Still slow:** DWG parse about 1.1 s; a zoom that crosses a detail band costs 20 to 40 ms per tick, mostly the
  triangle volume of text fill (10 M+ vertices at close zoom; glyph curves are already flattened to about 0.2 px).
  A GPU glyph atlas is the proposed fix and is not started. MynaOps and the Ask bar were not profiled.

## Update: editing a large drawing (2026-11-07)

Test drawing: the 3.5 MB DWG used above (26 206 entities, 710 250 primitives at full detail; text, hatches and block
references). Measured headlessly with the ignored probes `aad-host` `edit_latency_on_a_real_drawing`
(`AAD_PERF_FILE=<drawing> cargo test --release -p aad-host --lib edit_latency -- --ignored --nocapture`: the real
`Host`, everything the UI thread does per frame except the GPU upload) and `aad-cli` `edit_cost_by_operation`
(the scene rebuild alone). One machine, one run; use the figures for orders of magnitude.

| Operation | UI-thread frame before | After |
|---|---|---|
| move or copy 1 to 100 entities | at least 450 ms | about 43 ms |
| move or copy 2 000 entities | about 450 ms | about 62 ms |
| move or copy all 26 206 | 450 to 700 ms | 140 to 190 ms |
| erase 1, 100, 2 000 entities, and the undo of each | not measured | 42 to 56 ms |
| undo of an erase of everything | not measured | about 115 ms |
| select 100, 2 000 or all entities (engine + selection overlay) | not measured | under 6 ms |
| drag a copy of 2 000 or more selected entities: ghost per mouse move | 31 to 42 ms | 3 to 6 ms |
| one wheel-zoom step | 20 to 60 ms | mean 4 ms, worst 24 ms |
| first frame of the drawing (list and mesh) | 400 ms or more | about 180 ms |

- **Why an edit cost 450 ms:** `rebuild_scene` measured every entity's bounds three times on one thread (the
  viability check, the partition and the dirty pass; text, hatch and block bounds are the expensive ones) and then
  refused anyway. At full zoom-out the view-sized grid put the whole drawing in 3 tiles (the largest held 14 325
  entities, over the one-quarter limit), so any edit was "too much changed" and the caller did a full build.
- **What changed (`aad-render` `scene.rs`):** each entity's tile, bounds and inclusion are measured once, in
  parallel, and kept in `Scene::info`; an edit measures only what changed and reuses the rest, so the extents are
  exact again; the grid edge is halved while a tile holds more than `max(1000, n/24)` entities and the finer grid
  still fits under the tile cap (the next step is checked first, since a halving can quadruple the tile count);
  big tiles tessellate in parallel chunks; and the "too much changed" refusal is gone, because a rebuild of every
  tile now costs no more than a full build. The host asks `build_scene_if_viable` instead of `scene_tiling_viable`
  and then `build_scene`, which measured twice. The full scene build went from 363 ms to about 150 ms.
- **The copy ghost** (`aad-host` `overlay.rs`): the command previews at most 500 entities and rebuilt them at full
  detail on every mouse move. It now uses the main view's level of detail (text and hatches a few pixels tall are
  bars, tints or omitted).
- **Still open:** the frame after a small edit is about 43 ms, of which about 32 ms is the scene rebuild and the
  flatten into one list (an incremental flatten would remove most of it); the DWG parse is still about 1.1 s; a
  zoom that crosses a detail band still costs 20 to 40 ms per tick on a worker, mostly text fill. The Ask bar and
  MynaOps were not profiled. Numbers are from the headless host, not from timing the window.

## Update: startup and idle (2026-11-08)

Measured on the heavy 26 000-entity Working Drawing.dwg with `AAD_PERF=1` (new `PerfLog.Phase` markers write time since process start).

- **Startup:** creating the native host took 2.05 s of a 4.6 s launch. `assistant::Desk::new` called `osas_ai::status_label`, a blocking health check of the AI Core on loopback with a 2 s timeout, on the UI thread. With the suite launcher closed it waited for the timeout on every start. The check now runs on a background thread (`osas_ai::warm`), the shell reads the last known state, and asking the assistant does one fresh check (`core_ready_now`). `Host::create` is 0.5 ms. Window: 4.6 s to 2.5 s (heavy), 3.5 s to 1.4 s (small file); drawing loaded about 4 to 5 s after launch.
- **Idle CPU:** the 32 ms UI tick fetched and parsed the chrome and panels JSON repeatedly, each read built the JSON twice, and `panels_json` scanned every block reference each call. Now one read per tick, unchanged ticks skipped, the panels text cached in the host. Idle after load: 7.8 % to about 3 % of a core, the same for a small and a heavy drawing.
- The "settle" figure of 23 to 31 s in earlier open timings was the old idle noise defeating the script's idle test, not load time.
- Probes: `create_cost_by_part` (ignored test in `aad-host`) times the parts of host creation.

## Update: heavy drawings (2026-11-08)

Test file: a 115.7 MB DWG with 232,212 entities.

- Open (DWG read to scene ready): 16.8 s to about 9-10 s. Reading the DXF text now borrows sections and records instead of copying them, and a large ASCII DXF is cut at record starts and parsed on every core.
- Edit latency: 720 ms to about 205 ms per edit. A small edit patches the scene from the changed chunks of the entity store (`rebuild_scene_delta`) and builds mesh tiles in parallel.
- Still slow: `flatten_scene` (about 86 ms) and coarse mesh tiles (about 90 ms) per edit; move/copy of everything (about 1 s); the third-party DWG-to-DXF step (about 3 s).

## 2026-11-10: plot and picture export, and a repeatable audit

Measured on the 26k-entity `Working Drawing.dwg`, release build, machine at 7 % load, one process per figure (`tools/perf/audit.ps1` reads these the same way):

| | Before | After |
|---|---|---|
| PDF export (whole `aad-cli run <file> --export x.pdf`) | 13.3 s | 3.7 s |
| of which content stream (245 MB of text) | 4.0 s | 0.6 s |
| of which deflate | 7.5 s | 0.75 s |
| PNG export (2400 x 1600) | 5.2 s | 1.5 to 2.2 s |

How: the content stream is deflated in independent 4 MiB chunks on all cores (each chunk ends with a full flush, the checksums are combined; native only, wasm keeps the single stream); numbers are written into the stream without allocating; PNG export builds the display list at the detail the picture can show (curves flattened to a quarter pixel, small text greeked, fine hatches tinted). The decompressed PDF content is byte for byte what it was; the two PNGs were compared by eye.

The audit itself changed: `tools/perf/audit.ps1` checks the machine is quiet (and on AC power), copies the test file to a local folder, discards a warm-up run, takes N runs, reports min, median, max and spread (rerunning when the spread is above 1.3), appends every figure with the commit and machine state to `docs/qa/perf-history.jsonl`, and with `-Compare` checks medians against `docs/qa/perf-baseline.json` (warn above 1.10x, fail above 1.25x). The old "settled by CPU" open time is gone: the host now logs `first list build started`, `first list ready` and `first geometry frame` (with `AAD_PERF=1`), and the audit reads those. `aad-cli --version` prints the commit and the Myna release it was built for.

### Zooming a 232k-entity drawing (apw.dwg), 2026-11-10

The installed MynaCAD lagged on `apw.dwg`. With `AAD_PERF=1` each wheel zoom step stalled the UI thread for 137 to 405 ms (`mesh rebuild` on the UI thread: 4 to 6 giant tiles, so fingerprinting and remeshing used almost one core; a duplicate partition inside `mesh_tiling_viable`; a composed copy of every tile that only `ui.inspect` and tests read). Changes: lists of 30,000 or more primitives get finer tiles (`mesh_tile_edge_for`, aiming for about 6,000 primitives a tile, 19 to 52 tiles on apw) so every core has work; the partition is parallel and is made once (`tiling_plan`); tile fingerprints use a fast multiply-rotate hasher instead of SipHash; the composed mesh is not built when the mesh is tiled. Result, same file and machine: a zoom step is 59 to 105 ms on the UI thread (fingerprints 7 ms, remesh about 27 ms, partition 18 to 32 ms), down from 137 to 405 ms. Still open: opening the file takes about 8.7 s before the first frame (almost all of it reading the DWG), and every zoom step remeshes the whole drawing; meshing only the visible tiles, or on a worker thread, is the next step. The host log now also prints `pump:` and `mesh rebuild:` lines with `AAD_PERF=1`.

## Native working copy, autosave and backups (2026-11-11)

Design: [NATIVE-OPEN.md](NATIVE-OPEN.md), [AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md). Release build, headless host probe
`native_open_timing` (`AAD_PERF_FILE=<dwg> AAD_PERF_DIR=<scratch> cargo test --release -p aad-host --lib native_open_timing -- --ignored
--nocapture`), one run each on a machine that was also running other builds. "Open" is `app.open` (read, convert or decode, prepare, make the
document, fit); "first frame" is the first display list and mesh without the GPU.

| | Working Drawing.dwg (26 210 entities) | apw.dwg (232 212 entities, 115.7 MB) |
|---|---|---|
| Direct DWG read (every open before this change) | 1575 ms | 6837 ms |
| First open, converting to the working copy | 1143 ms | 8057 ms (DWG conversion 5520, encode 381, write 622) |
| Later opens from the working copy | **333 ms** | **2550 ms** (read and hash the source 87, decode 761, prepare and the rest about 1030, first frame 668) |
| `.osp` size after the import / after a save | 5.7 / 8.7 MB | 239.1 / 258.8 MB |
| Backup before a save (`VACUUM INTO`) | 27 ms | 619 ms |
| Save into the working copy (on the save worker) | 1.27 s | 9.7 s |
| Autosave: UI thread / first snapshot / after a one-entity edit | 0.09 ms / 46 ms / 8 ms | 0.13 ms / 1.86 s (128.6 MB) / 11 ms (26 KB) |

`aad-cli info` on apw: 6502 ms on the DWG, 2052 ms on its `.osp`. Fidelity on apw: 232 212 entities, 3 488 186 primitives, 6 112 531 line
vertices and 29 980 602 triangle vertices both ways, 0 of 3 840 000 PNG pixels different. Part 1 removes the DWG-to-DXF-to-parse cost
(about 5.5 s of the 6.8 s direct open on apw) from every open after the first. What remains on a warm open is the decode (0.76 s) and
`prepare` (flatten to 2D and the saved-state copy). The save time is not split further here; it writes the DWG bytes for MynaOps and older
versions as before, plus the native model and the backup.

### Zoom and pan on the worker (2026-11-11)

Headless probe `zoom_sweep_timing` (`AAD_PERF_FILE=<dwg> cargo test --release -p aad-host --lib zoom_sweep_timing -- --ignored --nocapture`)
on apw.dwg: 24 zoom notches in (x0.8 each) and 24 out about a point, then one drag of 20 moves; per step the UI-thread time of `Host::mesh`
(no GPU upload or present), then of the step that swaps in a list a worker built; then a one-entity move and its undo, ten times.

| | Before | After |
|---|---|---|
| Sweep step on the UI thread: p95 / max | 209.5 / 575.6 ms | 0.1 / 0.3 ms |
| Lists built synchronously on the UI thread during the sweep | 4 | 0 |
| Swapping in the worker's list: p50 / p95 / max | 50.6 / 64.1 / 79.3 ms | 0.1 / 0.1 / 0.1 ms |
| Edit and undo after the sweep (engine + mesh): p50 / max | 193.1 / 218.8 ms | 140.9 / 147.8 ms |

What changed in `Host::mesh` (`crates/aad-host/src/lib.rs`):
- A pan-cell change goes to the worker like a zoom-band change; before, the pan cell (which moves with every zoom notch, it is a multiple of the
  exact view height) forced a full display-list build on the UI thread, the 575 ms steps.
- The worker builds the list with its scene and also partitions, fingerprints and meshes its tiles (`premesh`) for the view it was asked for;
  the UI thread installs list, scene and tiles in one pump and frees the replaced ones on a thread. When the view has moved to another mesh
  origin by then, the tiles are remeshed as before.
- The scene is kept across band changes (it was dropped), so an edit after a zoom stays on `rebuild_scene_delta`.
- The mesh basis uses the band of the list on screen, not the band asked for, so the tiles shown are not remeshed while the next band is built.
- The tile edge is chosen once per list band from the list's density (`plan_tiles`, `FrameCache::tile_edge`), not per view octave.

Not measured here: the GPU upload of the swapped tiles in a window (the probe has no GPU), and input-to-present latency.

### Geometry database off the UI thread (2026-11-11)

Probe `geodb_timing` (`AAD_PERF_FILE=<dwg> cargo test --release -p aad-host --lib geodb_timing -- --ignored --nocapture`) on apw.dwg: a full
`geodb.build` takes 1.9 s (extract 0.3, scale check 0.6, classify 1.0 s); the incremental update after a one-entity move about 0.99 s, of which
`classify` in the touched region is 0.86 s (the region of one moved entity spans about 19 x 19 m on this drawing), context 68 ms, fingerprint
40 ms (`UpdateStats::stage_ms` now reports each stage). Both ran on the session (UI) thread.

`geodb.build` from the host (JSON, control channel, and typed without options) now runs on a worker: `aad_engine::cmd::geodb::begin_build` takes
the drawing and its model out of the session, `BuildJob::run` builds or updates and persists it on any thread, `finish_build` puts it back on a
later pump and echoes the usual line. UI-thread cost of the command: 0.1 ms (was about 990 ms); the result arrives about 990 ms later. Other
`geodb.*` commands are refused while a build runs; `{wait: true}` keeps the synchronous form for scripts. The classify cost of an incremental
update itself is unchanged.
