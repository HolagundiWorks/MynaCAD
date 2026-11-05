# Developer guide and rules for coding agents

O-Draft (2D drafting fork) is an open-source, pure-Rust 2D computer-aided drafting application for
architectural and civil work. It is a Windows-only program for now: macOS, Linux and the web are later phases. Start with [README.md](README.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and
[docs/status.md](docs/status.md) (what exists and what is only planned).

## Scope: 2D only
3D features, CAD parametric constraints (solver-based) and mechanical-engineering commands are out
of scope and have been removed (see [docs/status.md](docs/status.md)). A Grasshopper-style parametric
graph for walls and take-off exists separately ([docs/parametric.md](docs/parametric.md)); do not
add a constraint solver. DXF import projects 3D polylines and 3D faces onto XY, skips meshes,
flattens every entity, and reports 3D solids it cannot flatten ([docs/maintenance.md](docs/maintenance.md)).
Points keep a `z` field for DXF round-trip fidelity; do not add 3D behaviour or mechanical features.
Keep the general drafting, annotation, layer, block, hatch, layout and plot commands.

## Never crash
A crash loses someone's drawing. **This outranks feature work.**
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"` workspace default). Exceptions: `aad-host` (C ABI, Win32 child window, swap-chain); `aad-dwg` Job Object memory cap for the DWG worker (`job_win.rs` on Windows).
- **Errors are `Result<T, E>`** through the crate's error type and `?`. An unfinished feature returns an error or reports "not available yet"; it never panics.
- **Input-derived numbers are hostile** (DXF/DWG files, command-line text, MCP, control and agent parameters): `get()` not `[i]`, checked arithmetic, no NaN casts, cap input-sized allocations and loop counts.
- **Bound recursion** (nested or cyclic block references: `MAX_BLOCK_DEPTH`).
- **Last-resort guard:** `Session::execute` and every step of an interactive command run under `catch_unwind` (`crates/engine/src/guard.rs`); an escaped panic restores the drawing and reports an error.
- **Prove it:** every crash fix lands with a regression test (see `hostile_params_never_panic`, `hostile_dxf_does_not_panic`).
- Every production crate root carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.

## Non-negotiables
- **Clean-room.** Do not copy proprietary CAD vendor fonts (SHX), hatch patterns (`.pat`), linetypes (`.lin`), templates, CUI/PGP files, icons, artwork or help text from closed-source products. Interchange uses public DXF and DWG specifications and our own synthetic tests. Never copy or link GPL/LGPL/AGPL code (LibreDWG, LibreCAD, QCAD, SolveSpace, OpenCASCADE).
- **Assets.** Every icon is drawn in code (`crates/ui-egui/src/icons.rs`), every hatch pattern and linetype is our own definition (`crates/doc/src/library.rs`), the drafting font is our own (`crates/fonts/src/stroke.rs`). Any file asset must be original, public domain/CC0, OSI-licensed or redistributable Creative Commons, and must have a row in `ATTRIBUTION.md` (`cargo xtask assets` enforces it).
- **Fonts** are never committed. They are an optional build input (`CRAFT_FONTS_DIR`); code and tests must work without them.
- **Everything is a command.** User-visible behaviour is a command in `crates/engine/src/cmd/*` (`CommandSpec`: id, label, menu path, shortcut, aliases, params doc, `enabled`, JSON `run`, optional `interactive` prompt machine) with tests. UI-only commands live in `crates/ui-egui/src/menus.rs` (`UI_COMMANDS`). The command line, menus, toolbar, tool sets, scripts, CLI, control channel, MCP and the agent tools reach the same commands. New features reuse existing commands where possible.
- **Programmatic calls never open dialogs.** `engine.execute` runs the JSON form with defaults; only menu or toolbar invocation starts the prompt sequence.
- **Layering** is enforced by `cargo xtask layers`: L0 `geom`, standalone `dxf` / `dwg` / `vdb` / `param` / `llm` → L1 `color`, `doc` → L2 `fonts`, `render` → L3 `io` → L4 `engine` → L5 `ui-egui`, `host`, `mcp`, `agent` → apps. Nothing below L5 depends on egui/eframe/winit/wgpu/rfd. The UI crate is swappable. `aad-host` may use wgpu. The C# shell calls only that ABI.
- **The UI is thin:** panels read engine state and act through `app.run(id, params)`, `app.start(id)` and `app.cmdline(text)`. Colours come from `theme::Tokens`; there are two palettes (dark with a black drawing area, light with a white one) and no other colour customisation.
- **The AI never calculates or repairs.** The model only proposes calls to allow-listed tools; geometry, repairs and quantities come from deterministic engine commands ([docs/ai-agent.md](docs/ai-agent.md)).
- **The engine stays Rust.** The Windows shell may be C# in `apps/aad-winui`, calling `aad-host`. No handwritten JS/TS. The wasm check (`cargo xtask wasm`) stays in CI. The WinUI shell is Windows-only; do not add engine features that only build there.
- **Do not claim a feature works unless it exists and has a test.** Documentation separates implemented from planned ([docs/status.md](docs/status.md)).
- **Quality gate** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layering, licences, UI design system, wasm).

## Running and inspecting the app
- `cargo run --release -p aad -- --sample --control PORT` starts the app with a sample drawing and the control channel. Pick a free port.
- On Windows, the default `aad` binary (no eframe) starts the WinUI shell when `ODraft.WinUi.exe` is beside it or under `target/aad-winui` (or set `AAD_WINUI`), forwarding `--sample`, `--control` and drawing paths. Optional egui: `cargo build -p aad --features egui` then `aad --egui`. The status bar shows the loopback port (`--control` / `AAD_CONTROL_PORT`) for `engine.execute`, `cmdline.*`, `drawing.inspect` and `app.open` / `app.save` / `app.quit`. Build it with `cargo build -p aad-host`, then `dotnet build apps/aad-winui/Aad.WinUi.csproj -c Debug -p:Platform=x64 -o target/aad-winui` (copy `aad_host.dll` beside the exe). Open/Save/Save As and control `app.open`/`app.save` use workers (`SaveFileAsync` on the shell). Dual input channels: [docs/COMMAND_BAR.md](docs/COMMAND_BAR.md). Interactive pan/zoom path: [docs/PERFORMANCE.md](docs/PERFORMANCE.md). Window templates and shared components (every window goes through `StandardWindow.Present`; `cargo xtask ui` enforces it): [docs/UI_STANDARDS.md](docs/UI_STANDARDS.md). Tokens/sizing: [docs/UI_TOKENS.md](docs/UI_TOKENS.md), [docs/WINDOW_SIZING.md](docs/WINDOW_SIZING.md). `--egui` keeps the egui window (assistant UI and egui-only `ui.*` control methods).
- Drive it with JSON lines on `127.0.0.1:PORT`:
  - `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}` types at the command line like a user.
  - `{"id":2,"method":"engine.execute","params":{"command":"line","params":{"points":[[0,0],[10,0]]}}}` is the JSON form.
  - `{"id":3,"method":"ui.screenshot","params":{"path":"shot.png"}}` writes a PNG.
  - Methods: host control in `crates/aad-host` (WinUI) / `crates/ui-egui/src/control.rs` (egui); protocol: [docs/control-protocol.md](docs/control-protocol.md).
- For UI work, look at the result (screenshot). If no frame is presented (screen locked) use `ui.render` or `aad-cli`.
- Headless: `aad-cli run --sample --script 'LINE 0,0 10,10\n' --export out.png`; `aad-cli info file.dxf`; `aad-cli mcp`.
- Parallel agents: use a separate `CARGO_TARGET_DIR` per agent and edit only the crates you own.

## Documentation rules
Full policy: [docs/documentation-policy.md](docs/documentation-policy.md). Summary:
- English only. Technical documentation only: architecture, APIs, configuration, installation, testing and maintenance. No marketing or promotional text.
- Documentation must reflect reality: never claim a feature works unless the implementation exists and has been verified (tests run, not merely written). Update code, tests and docs in the same change.
- Assess documentation impact on every change. Update every affected canonical page, not only the nearest one. Capability matrix: [docs/status.md](docs/status.md) (implemented vs planned; use the status vocabulary in the policy when refining rows).
- One canonical source per subject; do not maintain contradictory copies. Regenerate derived indexes (`cargo xtask parity` → `docs/parity-checklist.md`); never edit generated output by hand.
- Before reporting completion: list affected docs, disclose unrun tests and known limitations, and distinguish done work from follow-ups.
