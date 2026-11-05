# O-Draft canvas rendering: review and roadmap

Source plan: *ODraft canvas rendering review and plan* (Oct 26, 2026).  
Branch work: `fix/canvas-phase3-gpu-curves` (Phases 1–2 landed earlier).

The canvas already runs on the GPU (wgpu on Direct3D 12) with analytic anti-aliased strokes. It does **not** need a new engine. The rough look comes from specific gaps listed below. Phase 1 is small changes inside the existing renderer; Phase 2 reworks strokes, text and hatches; Phase 3 moves curves onto the GPU.

## How the canvas renders today

Every frame goes through one shared path (WinUI host and optional egui shell). The engine never draws pixels itself; it hands the GPU a pre-tessellated mesh.

1. **Display list** (`crates/render/src/lib.rs`) — entities → world polylines, filled triangles or points. Curves flatten at ~0.2 screen-pixel chord tolerance per zoom octave (`tessellation_band`).
2. **Hatches** (`hatch.rs`, `fill.rs`) — solid/gradient via slab triangulation; patterns as short hairline segments.
3. **Text** (`crates/fonts`) — stroke font as polylines; TrueType via `skrifa`, fixed 6/8 points per curve, ear-clip fill. No hinting, no glyph atlas.
4. **Mesh** (`crates/render/src/mesh.rs`) — fills as triangle lists; every polyline segment an instanced screen-space quad (width in points). Positions f32 relative to a snapped origin.
5. **GPU** (`crates/aad-host/src/gpu.rs`) — wgpu prefers DX12. WGSL expands segment quads and uses distance-to-segment coverage AA. Fills were solid with MSAA off; swapchain is sRGB (linear-light blend).
6. **Caching** (`aad-host`) — display list keyed by revision, zoom band, LOD, space, and (Phase 1) model pan cell; optional tiled `Scene` (Phase C) with dirty-tile rebuild. Layout VP model LRU is process-wide (not TLS). Pans update a uniform. Zoom-band changes rebuild on a worker.
7. **Export** (`raster.rs`) — tiny-skia with AA on (often cleaner than the live canvas).

## Problems (eight)

| # | What you see | Cause |
| --- | --- | --- |
| 1 | Jagged TrueType text, solid hatches, wide fills | Fills full coverage; `SAMPLE_COUNT` was 1 |
| 2 | With LWT on, 0.30 / 0.35 mm thinner than 0.25 mm | `wide_width`: ≤1 pt fell back to 1.5 pt hairline while 0.30→1.13 pt |
| 3 | Dark lines on white look thin / washed out | Coverage blended in linear light (sRGB swapchain) |
| 4 | Circles/arcs heavier and beaded vs straight lines | One quad per segment with round caps; joints double-blend |
| 5 | Big text faceted when zoomed in | Glyph curves fixed 6/8 steps, independent of zoom |
| 6 | Zoom jumps ~1.5×; momentary polygons; tearing while pan | Instant wheel factor; async retessellation; `AutoNoVsync` |
| 7 | Hatch/text pop; moiré when zoomed out | Hard LOD at 25 / 50 wpp; dense patterns full contrast |
| 8 | After deep zoom + far pan, geometry missing until next zoom/edit | Model cull around view centre; cache key omitted centre |

## Decisions (open questions resolved)

| Question | Decision |
| --- | --- |
| Lineweight look | **Screen pixels** in model space (compact CAD look; not zoom-scaled). True paper scale is a Phase 2 layout mode. |
| Vsync default | **On** (`AutoVsync`). Opt out with `AAD_VSYNC=0`. Smoothness wins over one frame of cursor lag. |
| Vello | Prototype only after Phase 1–2, if custom shaders still fall short on sheets. Env flag: `AAD_CANVAS=vello` (L5 `aad-vello` / `vello_cpu` PNG path for control `ui.render` only; interactive mesh unchanged). |

## Roadmap

### Phase 1 — small, low-risk (most visible gain)

| # | Change | Fixes | Effort | Status |
| --- | --- | --- | --- | --- |
| 1 | 4× MSAA (`SAMPLE_COUNT = 4`) on WinUI GPU path; egui when the paint target allows | #1 | 0.5 d | **Done (WinUI).** egui callback shares egui’s single-sample pass — deferred |
| 2 | Monotonic `wide_width` over the full `Lineweight::STANDARD` list; hairline is the floor | #2 | 0.5 d | **Done** (floor at `HAIRLINE_POINTS`; full-list test) |
| 3 | Gamma-aware AA: coverage contrast curve when linear-out (or UNORM target later) | #3 | 1–2 d | **Partial** (coverage `pow(…, 0.45)`); UNORM path still open for visual tuning |
| 4 | Vsync on by default; `AAD_VSYNC=0` opt-out | tearing in #6 | 0.5 d | **Done** |
| 5 | Smooth zoom: ~120 ms eased animation, accumulate notches; softer per-notch factor | stepped zoom in #6 | 2 d | **Done** (host animates when GPU live; tests snap to end) |
| 6 | Cull + cache include mesh origin / pan cell; rebuild when the view leaves the cell | #8 | 0.5 d | **Done** |

### Phase 2 — strokes, text, hatches

| # | Change | Fixes | Effort | Status |
| --- | --- | --- | --- | --- |
| 7 | Joined stroke strips (butt edges + round join/cap discs; negative instance width = butt) | #4 | ~1 w | **Done** (`stroke_polyline` in `mesh.rs`) |
| 8 | Flatten glyphs by zoom tolerance; cache per glyph × octave | #5 | 2–3 d | **Done** (`with_world_chord_tol` + flatten band in glyph cache) |
| 9 | Density fade for hatches/text instead of hard LOD cut-offs | #7 | 3 d | **Done** (`LodFlags` greek bars + hatch `fill_tint`; host/egui wire `lod_flags_for_view`) |
| 10 | Lineweight modes: screen (model) + true scale (layouts) | presentation | 2 d | **Done** (`LineweightMode` + shader `view.c.y`; paper + LWDISPLAY → true scale) |

### Phase 3 — GPU curves / optional Vello

| # | Change | Fixes | Effort | Status |
| --- | --- | --- | --- | --- |
| 11 | GPU arcs/circles/ellipses + Loop–Blinn-style glyph fills | sharp at every zoom | 3–4 w | **Partial** — circles/arcs as SDF rings (`Kind::Arc`); ellipses and non-uniform-scaled circles as conjugate-diameter rings (`Kind::Ellipse` → same `MeshData::rings` / `vs_ring`). Continuous linetype only; dashed styles and degenerate axes stay polylines. CPU raster / clip tessellate rings via `sweep_radians` (avoids `f32::TAU` collapsing a full circle). Phase D ships CPU glyph×band triangulated retention first; **still open:** GPU atlas / Loop–Blinn glyph fills |
| 12 | Vello prototype for paper / plot preview behind a flag | quality compare | ~1 w | **Prototype (unmeasured)** — `aad-vello` (L5) + `AAD_CANVAS=vello` selects `vello_cpu` for host/egui/MCP `ui.render` PNG only. Default unset → tiny-skia. Interactive mesh/wgpu untouched. Licence: Apache-2.0 OR MIT. **Not claimed:** sheet quality win vs custom path; GPU `vello` interactive paper; replacing Phase A–D |

### Phase 3 (#11 curves) test commands

```text
cargo test -p aad-render --lib mesh::tests
cargo test -p aad-render --test lod_and_frustum
cargo test -p aad-ui-egui --lib gpu::tests
```

### Phase 3 (#12 Vello spike) test commands

```text
cargo test -p aad-vello --lib
cargo xtask layers
cargo xtask licenses
# Default path (flag off) still uses tiny-skia; wasm builds ui-egui without requiring AAD_CANVAS=vello:
cargo xtask wasm
```

**How to evaluate quality (manual, not claimed here):** build a nameplate / layout sheet, export or
`ui.render` PNG twice — once with `AAD_CANVAS` unset (custom tiny-skia) and once with
`AAD_CANVAS=vello` — then compare AA on arcs, text, and dense hatches. Keep the interactive canvas
on the custom mesh path either way.

## Performance (measure each change)

From `docs/PERFORMANCE.md` (Oct 2026 machine). Re-measure with `aad-cli perf` / `perf_probe` before and after.

| Hotspot | Today | Direction |
| --- | --- | --- |
| Display-list at extents | 13 ms / 10k, 129 ms / 100k | GPU curves (#11, ellipses landed); per-entity×octave tessellation cache; **Partial (Phase C)** — tiled `Scene` / dirty-tile `rebuild_scene` + frustum skip of far tiles (`scene_tiles`); absolute ≤ 80 ms @ 100k not claimed |
| Text-heavy rebuild | 0.32–0.38 s | Glyph outline cache (#8); greek bars (#9); **Partial (Phase D)** — CPU glyph×band triangulated mesh cache + hatch pattern expansion cache (`text_hatch_retention`); **proposed** GPU atlas / Loop–Blinn |
| Any edit | Full `build_mesh` + new GPU buffers | **Partial (Phase B+C)** — spatial mesh tiles remesh + WinUI dirty tile upload; dirty display-list tiles via `rebuild_scene`; monolithic fallback for tiny tile counts / spanning wipeouts |
| Layout VP pan across cell | 52 ms / 100k | Existing LRU; GPU curves help |
| GPU frame time | Unknown | Host/egui `ui.inspect` `perf.buildMs` / `meshMs` / `rings` / `meshBytes` (implemented). Optional WinUI timestamp queries still open |

## Verification

Each Phase 1 change has a unit test. Before/after screenshots via `ui.screenshot` on Windows (or CI). Cloud/Linux agents cannot run the WinUI shell.

### Phase 1 test commands

```text
cargo test -p aad-render --lib mesh::tests
cargo test -p aad-render --test lod_and_frustum
cargo test -p aad-host present_mode_defaults_to_vsync
cargo test -p aad-host sample_line_and_wheel_zoom
cargo test -p aad-host fine_pan_keeps_the_cached_mesh
cargo test -p aad-host edit_only_remeshes_dirty_tiles
```

### Phase 2 (#9–#10) test commands

```text
cargo test -p aad-render --lib mesh::tests
cargo test -p aad-render --test lod_and_frustum
```
