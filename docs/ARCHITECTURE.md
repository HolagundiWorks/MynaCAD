# Architecture

How the code on `main` is built. Planned work is in [status.md](status.md). The rules that keep this
shape (layering, never-crash, everything-is-a-command, clean-room) are in [CLAUDE.md](../CLAUDE.md).

## Workspace

`cargo xtask layers` enforces the layering: nothing below L5 depends on egui, eframe, winit, wgpu or rfd.
`apps/aad-winui` is C# and is excluded from the Cargo workspace.

| Layer | Crate | What it does |
|---|---|---|
| L0 | `geom` | f64 vectors and matrices, bounds, lines, arcs and bulges, circles, ellipses, B-splines, intersections, tessellation, planar region tracing for HATCH and BOUNDARY |
| L0 | `dxf` | Standalone DXF tag reader and writer (ASCII and binary), tag-count limits for hostile input |
| L0 | `odp` | The `.osp` project file (SQLite): drawing, take-off, project record, markups and drawing history; the web build uses only its history model. See [PROJECT-FILE.md](PROJECT-FILE.md) |
| L0 | `param` | Standalone parametric graph: typed nodes and ports, units, formulas, validation, incremental evaluation. No drawing I/O ([parametric.md](parametric.md)) |
| L1 | `color` | Colour index table, true colour, ByLayer and ByBlock, display colour against the background |
| L1 | `doc` | The drawing database: header, symbol tables, blocks, model space, layouts; copy-on-write `EntityStore`; hatch pattern and linetype library (`library.rs`) |
| L2 | `fonts` | Single-stroke drafting font, TrueType/OpenType outlines through `skrifa`, text and MTEXT layout, font fallback |
| L2 | `render` | Display lists (linetypes, hatches, text, blocks, dimension geometry, paper-space clipping); layout viewport model-tessellation LRU; LOD flags and optional model frustum; CPU rasteriser for PNG, plot preview and tests ([PERFORMANCE.md](PERFORMANCE.md)) |
| L3 | `io` | DXF to document mapping (`dxf_read.rs`, `dxf_write.rs`, MynaCAD extension data in `dxf_ext.rs`), PDF plotting with the drawing layers as optional-content groups, SVG export with layer groups, PNG export including a layout at a chosen dpi, PDF page reading for `underlay.*` and `pdf.import` |
| L3 | `office` | Loads `office.dll` at run time (Windows) and reads the company file for the `companyfile` command |
| L3 | `dwg` | DWG through a DXF bridge: the `acadrust` crate (MPL-2.0, unmodified) converts DWG and DXF bytes; native builds only. The apps run the conversion in a worker process (`isolated.rs`) so a damaged file cannot abort the program; on Windows the worker has a Job Object memory cap (`job_win.rs`) |
| standalone | `llm` | Offline language-model runtime facade (Qwen3 prompt format, model file check). The llama.cpp backend is the off-by-default `llama` feature (`aad-host` / `aad` / `aad-cli` `--features assistant`) ([ai-agent.md](ai-agent.md)) |
| L4 | `engine` | `Session` (including disposable `param_cache`), the command registry (`cmd/*`, including `param.*`), command-line parser and prompt machines, object snaps, selection, grips, associativity, system variables, units, undo, audit (`audit.rs`), flatten on import (`flatten.rs`) |
| L5 | `ui-egui` | The egui front end: GPU canvas, menus, command line, palettes, dialogs, icons drawn in code, light/dark theme, interface string catalog. Used by `aad --egui`, the control channel, the assistant and the web build |
| L5 | `host` | Windows shell host: a session, cached display list / GPU mesh (async zoom-band rebuild), pointer clicks, pan, wheel, snaps, grips, dynamic input, the registry menu, the command line, form descriptions, a revision-tagged properties/layers/blocks snapshot, the last command result, the assistant step machine (it may call `agent`; `agent` does not call `host`), a loopback JSON control channel (open/save replies deferred across `pump`), and the C ABI `apps/aad-winui` calls. The drawing is a Win32 child because wgpu 30 has no SwapChainPanel surface |
| L5 | `mcp` | MCP server (JSON-RPC 2.0 over stdio) on a headless session or bridged to a running app |
| L5 | `agent` | Tool layer for a local language model: registry, validation, confirmation, loop limits, audit log ([ai-agent.md](ai-agent.md)) |
| L5 | `auditor` | Auditor v0 detect-only: drawing audit/validate wrappers, command/menu parity, WinUI token scan, combined JSON report ([ai-agent.md](ai-agent.md), [TESTING.md](TESTING.md)) |
| L5 | `vello` | Optional Phase E quality spike: `vello_cpu` (Apache-2.0 OR MIT) PNG path for control/`ui.render` when `AAD_CANVAS=vello`. Not the interactive canvas; must not move into `aad-render` ([canvas-smoothness.md](canvas-smoothness.md), [PERFORMANCE.md](PERFORMANCE.md)) |
| app | `aad` | Desktop app. On Windows the default binary only launches `aad-winui` (no eframe). Optional `aad --features egui -- --egui` keeps the eframe window; macOS/Linux/BSD always link egui/eframe |
| app | `aad-winui` | Unpackaged WinUI 3 shell over `aad-host`: registry menus, command line, dynamic input, grips, snaps, form dialogs, file pickers, plot preview (nameplate PDF), calculator, Pomodoro, panel pin and float, Project / Properties / Layer / Blocks side tabs, and resizable Classic/Pro panels around the child drawing window |
| app | `aad-cli` | `info`, `convert`, `run` (scripts), `commands`, `sample`, `mcp`, `auditor` |
| app | `aad-web` | WASM build of the same UI (no DWG) |
| tool | `xtask` | `ci` (fmt, clippy, tests, assets, layers, licences, ui, wasm), `layers`, `assets`, `licenses`, `ui`, `wasm`, `parity`, `stats`, `logo`, `corpus` |

## Data model

- A `Drawing` is header variables, symbol tables, block definitions, model space and paper-space
  layouts. Entities carry a handle, layer, colour, linetype, lineweight, transparency and an
  `EntityKind`: Line, Point, Circle, Arc, Ellipse, LwPolyline, Spline, Ray, XLine, Text, MText,
  AttDef, Insert (with attributes), Dimension, Leader, MLeader, Hatch (pattern, solid, gradient),
  Solid, Trace, Viewport, Image, Wipeout, Table and Unknown (raw DXF tags).
- 3D polylines and 3D faces are not entity kinds: the reader projects them to 2D. 3D solids,
  surfaces and meshes are kept as `Unknown` and reported on import ([maintenance.md](maintenance.md)).
- Points keep a z coordinate so that DXF round-trips; drafting is 2D in the world coordinate
  system and there is no UCS.
- Entity collections are copy-on-write, so undo is a whole-drawing snapshot that costs little.
- Table entities read from files are displayed, but there are no table commands.

## Commands

Every user-visible action is a `CommandSpec` (`crates/engine/src/cmd/mod.rs`): id (the command name
in lower case), label, menu path, shortcut, aliases, a JSON `run`, an `enabled` check and an
optional interactive prompt machine. Count with `aad-cli commands` (includes `param.*`; see
[parametric.md](parametric.md)). UI-only commands live in `crates/ui-egui/src/menus.rs`. The
command line, menus, toolbar, tool sets, scripts, CLI, control channel, MCP and the agent tools all
reach the same commands. JSON calls never open dialogs.

## File import

`open`, drag and drop and `recover` read a file through the `IoHooks` installed by the host, then
call `cmd::file::import_drawing`, which flattens a working copy to 2D and keeps the original as the
saved state and one undo step. Saving keeps the first replaced file as `<name>.bak`. See
[maintenance.md](maintenance.md).

## Rendering

`render::build` turns a space into a display list once per change; the UI uploads it to wgpu in
batches (`gpu.rs`). Tessellation tolerance follows the view. Model space can also retain a tiled
`Scene` (`aad_render::scene`: `Arc` per world-grid tile, dirty-tile `rebuild_scene`, flatten for
draw order); mesh packing may remesh only dirty spatial tiles (Phase B). Filled TrueType glyphs
reuse a process-wide font-unit triangulated mesh cache (`aad_render::glyph_mesh`); hatch pattern
sweeps reuse a capped expansion cache (Phase D). The R-tree (`spatial.rs`) is patched per changed
store chunk, so picking stays fast on large drawings. PNG, PDF, SVG and plot preview reuse the same
display list through the CPU rasteriser or vector writers. An optional L5 `aad-vello` path
(`AAD_CANVAS=vello`) can rasterise the same display list with Linebender `vello_cpu` for
paper/plot PNG compare; default remains tiny-skia, and the interactive mesh/wgpu canvas never
switches. The drawing-area background is black in the dark theme and white in the light theme;
colour 7 is drawn white or black to contrast.

## Files

DXF is the native path: `dxf` parses tags, `io::dxf_read` maps them to the document, and
`io::dxf_write` writes an R2000 (AC1015) DXF. DWG is converted to DXF in memory by `acadrust` and
read through the same path; saving DWG writes DXF and converts it back. AAD-only data
(associativity details, layer states, plot style tables, table flags, hatch gradients) travels in
extension data under the application name `AAD`. Object types MynaCAD does not model are kept
as raw data when read and are not written back; saving reports them.

## Automation

- Control channel: `aad --control PORT` accepts JSON lines (`cmdline.input`, `engine.execute`,
  `ui.screenshot`, `ui.render`, ...); see [control-protocol.md](control-protocol.md).
- Trust boundary (loopback only, no AuthZ today): [threat-model.md](threat-model.md).
- MCP: `aad-cli mcp` (headless) or `--connect HOST:PORT` (the running app); see [mcp.md](mcp.md).
- CLI: `aad-cli run --script ... --export out.png` for batch work.
- Agent tools: `crates/agent`; see [ai-agent.md](ai-agent.md).

## Safety

Production crates deny `unwrap`, `expect` and `panic!` and forbid `unsafe`. `Session::execute` and
interactive input run under `catch_unwind`, restoring the drawing if a panic escapes. Input-sized
allocations are capped (DXF tag counts, DWG size limits, hatch segment and island limits,
`MAX_BLOCK_DEPTH`, control JSON-line size). The quality gate is `cargo xtask ci`.

## Undo and redo

`Session::execute` keeps the drawing as an `Arc<Drawing>` before each undoable command and pushes a
`Snapshot` (label, previous drawing, previous selection) when the command changed it. Because the
entity store is copy-on-write in chunks of up to 1 024 entities, a snapshot shares every chunk the
command did not touch. History is capped at 2 000 steps per document. Commands marked `noundo`
(for example save) do not add a step. After a command, edits that would leave NaN or infinite
coordinates are refused and the drawing is restored (`finite.rs`).

## Threading

The UI, the engine and rendering run on the thread that draws the window. Threads that exist:

- `spatial::warm` builds the picking index in the background when the display list is rebuilt
  (`aad-spatial-warm`, one at a time; not on wasm).
- The DWG worker process and its feeder and reader threads (`dwg/src/isolated.rs`).
- The control-channel server (one thread per connection, `apps/aad/src/control_server.rs`),
  which hands requests to the UI thread over a channel.

File open, save and plot run on the UI thread; there is no progress state yet
([UI_BUG_BACKLOG.md](UI_BUG_BACKLOG.md), UI-003).

## External dependencies

Rendering and windowing: `egui`/`eframe` 0.36, `egui-wgpu`, `wgpu`. Geometry and spatial queries:
`rstar`. Fonts: `skrifa`. DWG: `acadrust`. Local model (optional): `llama-cpp-2`. Tests and
benchmarks: `egui_kittest`, `proptest`, `criterion`. The full list is in `Cargo.lock`.
