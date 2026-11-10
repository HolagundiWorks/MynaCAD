# MynaCAD

MynaCAD (formerly AAD, Accelerated Architecture Drafting) is the drafting app alongside **MynaHub** (office and suite hub) and **MynaOps** (quantities, estimates and billing). It is a 2D computer-aided drafting application written in Rust, aimed at architectural and civil
drafting. It reads and writes DXF and DWG and can be driven from scripts and agents. **Windows is
the only supported platform for now**; macOS, Linux and the web build come in later phases (the
code stays portable, but only Windows is built, tested and packaged). This repository is a fork of
[storytold/cadcraft](https://github.com/storytold/cadcraft) with the 3D, parametric-constraint and
mechanical features removed.

> **The suite:** names, roles, file formats, toolbars of every app and how they link in real time are in [docs/SUITE.md](docs/SUITE.md).

What is built and what is planned is listed in [docs/status.md](docs/status.md). Nothing in the
documentation describes a feature as available unless it exists and has tests.

## The interface

![MynaCAD opens on a welcome screen with New drawing, Open drawing and Project Wizard](docs/images/draft-welcome.png)

**Welcome.** MynaCAD opens here until you start or open a drawing: **New drawing**, **Open drawing...** or **Project Wizard...**. Projects are chosen in Myna; MynaCAD opens on the one Myna locked, and recent drawings are listed underneath.

![The MynaCAD workspace: tool strip, canvas, side panels and status bar, with the parametric footing under the pointer described in Properties](docs/images/draft-workspace.png)

**Workspace.** From the top of the window down:

| Area | What it is |
|---|---|
| Title bar and tabs | One drawing per window (the open drawing's tab), and a **Take-off** tab that opens the quantity take-off toolbar (walls, openings, columns, beams, floors, phases). |
| Menus | File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, **Architecture**, Help, Window. Every item is an engine command, so the command line, scripts, MCP and the AI assistant run the same code. |
| Layer bar | The current layer. |
| Tool strip (left) | Draw, modify and annotate tools in two columns, then the architecture group (doors, windows, rooms, axis grid, stairs) and the **structural tools**: RCC column, Round column, RCC beam, Footing, Slab and Lintel. |
| Canvas | The drawing, drawn on the GPU. **Design** and **Drawing tabs** switch between the model and the sheets; the level bar underneath holds the levels (L0, +) and the Ask bar floats at the bottom. |
| Side panels (right) | **Properties**, **Layers**, **Blocks**, **Project**, **History** and **Revisions**, chosen from the rail on the right edge. Properties shows what is under the pointer; for a parametric block it lists the type and every parameter. |
| Status bar | Pointer coordinates and the drafting switches (object snap, ortho, polar, snap tracking, dynamic input), lineweight and transparency display, drawing units, the layer manager, plot preview, the theme, and the state of the AI assistant. |

**Ask bar.** The pill at the bottom of the canvas takes plain-language requests for the local AI agent (Olmo 3 7B on this computer), hosted by MynaOps: when no MynaOps hosts it, MynaCAD starts MynaOps without a window on the first question and shows "starting MynaOps…" meanwhile. It asks before it changes anything (Allow or Decline). Typed commands (`line`, `zoom`, `smartblock.insert ...`) go to the command line, which opens as you type.

### Structural details

![Structural details drawn by the reinforced concrete tools: column, beam, stirrup, isolated and combined footings, beam elevation and lift raft](docs/images/structural-details.png)

The Architecture bar's structural tools place **parametric reinforced concrete blocks** and the reinforcement is edited in the Properties panel, not in a dialog. They are drawn in the style of Indian structural drawings: bars coloured by diameter (corner bars red, others magenta or blue), hooked ties and cross-ties, dimensions in feet and inches (or millimetres), leaders with schedule notation (`4Y20 + 6Y16`, `Ties-Y8@6"`, `Y10@5"`), PCC and boulder soling under footings with the development length Ld to IS 456, and stirrup zones on a beam elevation (`5 nos @ 6"`, `@ 7"`).

Parameters follow IS 456:2000: concrete and steel grade, exposure (Table 3), cover, bar and tie diameters and spacings. Below the parameters, Properties shows whether the member **complies** and, for each broken limit, the clause. When the drawing is saved into a project (`.osp`), the details are recorded for MynaOps, which shows them on the element pages (read only). See [docs/smart-blocks.md](docs/smart-blocks.md).

![The Blocks panel lists the drawing's blocks as thumbnails](docs/images/draft-blocks.png)

The **Blocks** panel shows this drawing's blocks and the library (steel sections, plumbing, HVAC, electrical, interior) as thumbnails; click one and pick a point. The reinforced concrete members are not listed there: they are tools on the Architecture bar.

## Status

Version 0.6.0. Windows x64 is the only platform that is built, tested and packaged. Working today:
2D drafting and modifying, DXF and DWG, layouts and plotting, the architecture tools, a command line
and automation interfaces. Not available: 3D, parametric constraints, UCS, SHX fonts, and the local
language model in the apps (the runtime crate exists, see [docs/ai-agent.md](docs/ai-agent.md)).
The full list of what exists and what is planned is in [docs/status.md](docs/status.md); known
problems are in [docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md).

## Prerequisites

Rust stable with the MSVC target (1.95 or later for the desktop and UI crates; the other crates
declare 1.90) and the Visual Studio Build Tools. A GPU with a wgpu-compatible driver runs the
application. Nothing else is needed to build and test. Installing the toolchain, packaging and
troubleshooting: [docs/BUILD_AND_DEVELOPMENT.md](docs/BUILD_AND_DEVELOPMENT.md).

## Build and run

```sh
cargo build -p aad-host
dotnet build apps/aad-winui/Aad.WinUi.csproj -c Debug -p:Platform=x64 -o target/aad-winui
cargo run --release -p aad -- --sample              # WinUI shell (default Windows binary; no eframe)
cargo run --release -p aad -- file.dxf              # forwards the path to WinUI
cargo run --release -p aad --features egui -- --egui --sample   # optional egui window
```

The WinUI shell needs the .NET 8 SDK. The default Windows `aad` binary (shipped as `o-draft.exe`) does not link eframe; rebuild with `--features egui` for `aad --egui` (widget automation). Copy `target/debug/aad_host.dll` next to `ODraft.WinUi.exe` after building the host.

Type commands at the command line, for example:

```text
line 0,0 @10,0 @0,5 c
circle 5,2 1
offset 0.25
zoom e
```

The web build (`apps/aad-web`) is not part of the supported product yet; it is only checked to compile.

## Tests

```sh
cargo xtask ci        # fmt, clippy -D warnings, tests, asset list, layering, licences, UI rules, wasm check
```

`cargo xtask --help` lists the other tasks (`layers`, `assets`, `wasm`, `parity`, `version`, `logo`, `stats`).

## Command-line tool, MCP and control channel

```sh
aad-cli info drawing.dxf
aad-cli convert drawing.dxf drawing.svg
aad-cli run --sample --script 'CIRCLE 22,3 1\n' --save out.dxf --export out.png
aad-cli mcp                               # MCP server on stdio, headless
aad-cli mcp --connect 127.0.0.1:7979      # ...or bridged to a running app (start it with --control 7979)
```

Image exports (PNG 2400x1600, SVG, PDF) fit the model-space extents; `--window x0,y0,x1,y1`
frames an exact rectangle and `--view extents|current` forces either framing. The JSON control
channel is described in [docs/control-protocol.md](docs/control-protocol.md) and the MCP tools in
[docs/mcp.md](docs/mcp.md).

## Opening files

Every DXF or DWG that is opened is flattened to 2D on a working copy, and the original stays
recoverable. `PURGE`, `AUDIT` and `RECOVER` keep a drawing healthy. Details, limits and safety
measures: [docs/maintenance.md](docs/maintenance.md).

## Built-in architecture tools

Doors and windows (Architecture bar; optional plan block, opening width on the drawing feeds the
schedule), walls (`wall.draw` on the command line), grid, columns, lintels, stairs, lifts, levels
and section markers, quantity take-off with rates and a bill, electrical layout, and Project
Settings (details, default slab thickness and beam depth) are built-in under the Architecture menu
([docs/hcw-kit-port.md](docs/hcw-kit-port.md)). Print and plot preview fit a model window into
Nameplate L or Nameplate P ([docs/sheets.md](docs/sheets.md)). A calculator for feet, inches,
lengths, areas and volumes and a Pomodoro timer are floating widgets under Tools
([docs/widgets.md](docs/widgets.md)).

## Local AI assistant

An offline assistant can call a fixed set of validated tools (summary, flatten, audit, purge, recover, units, plot configuration). It runs a local Qwen3-1.7B model through llama.cpp and is **not part of the default build**: build with `--features assistant` (`cargo build --release -p aad --features assistant`; needs CMake, a C++ toolchain and LLVM) and put the model file in `%APPDATA%\MynaCAD\models`. Type a request in the command bar (a leading `/`, a `/reference` such as `/layers`, or a plain sentence; see [docs/COMMAND_BAR.md](docs/COMMAND_BAR.md)), or use `aad-cli ask "request"`. A tool that changes the drawing asks first. The Windows **AI installer** bundles the runtime and model: `powershell -NoProfile -ExecutionPolicy Bypass -File packaging/windows/package.ps1 -Arch x64 -Assistant` (needs the model at `%APPDATA%\MynaCAD\models\Qwen3-1.7B-Q4_K_M.gguf` or `AAD_MODEL_FILE`). Output: `dist/release/o-draft-ai-<version>-windows-x64.msi` and a portable zip. The plain installer (`package.ps1` without `-Assistant`) stays drafting-only. Both are open source (MIT OR Apache-2.0); the wizard shows `packaging/windows/EULA.rtf`. See [docs/ai-agent.md](docs/ai-agent.md).

## Configuration

- Fonts: MynaCAD ships none and uses the TrueType/OpenType fonts installed on the system. SHX fonts
  are not read; text in an SHX style is drawn with the built-in stroke font. `FONTALT` and
  `FONTFALLBACK` (or `AAD_FONTALT`, `AAD_FONTFALLBACK`) name a replacement font and the
  fonts tried first for characters a font lacks. `.` clears either.
- Logs: the desktop app writes to standard error and to `logs/aad.log` in the settings
  directory (`~/.config/aad/` on Linux and the BSDs, `~/Library/Application Support/AAD/`
  on macOS, `%APPDATA%\MynaCAD\` on Windows). `AAD_CONFIG_DIR` overrides the directory and
  `RUST_LOG` sets the levels (default `info` for MynaCAD's crates, `warn` for the rest). Each launch
  rotates the previous log; the file stops growing at 16 MiB.
- Theme: one light/dark toggle (View menu, status bar). Dark has a black drawing area, light a
  white one. There is no other colour customisation.
- Interface language: English only.

## Documentation

| File | Contents |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, layering, data model, commands, undo, threading, rendering, files |
| [docs/PROJECT_STRUCTURE.md](docs/PROJECT_STRUCTURE.md) | Directory and module map, entry points, generated folders |
| [docs/BUILD_AND_DEVELOPMENT.md](docs/BUILD_AND_DEVELOPMENT.md) | Toolchain, build, run, test, packaging, troubleshooting |
| [docs/CAD_ENGINE.md](docs/CAD_ENGINE.md) | Entities, geometry, units, snapping, selection, hatch, file interoperability, limits |
| [docs/UI_UX.md](docs/UI_UX.md) | Screens, menus, mouse and keyboard, dialogs, window sizes, UI test coverage |
| [docs/TESTING.md](docs/TESTING.md) | Test layers, property and mutation tests, regression procedure, CI |
| [docs/PERFORMANCE.md](docs/PERFORMANCE.md) | Benchmarks, measurements, bottlenecks, optimisation history |
| [docs/RELEASE_AND_PACKAGING.md](docs/RELEASE_AND_PACKAGING.md) | Release build, installer, validation, artifact retention |
| [docs/KNOWN_ISSUES.md](docs/KNOWN_ISSUES.md) | Confirmed issues, hypotheses, limits |
| [docs/ACTIVE_DRAWING_SPACE.md](docs/ACTIVE_DRAWING_SPACE.md) | Drawing spaces, layer protection, context bar, Classic and Pro workspaces: design, commands, acceptance status |
| [docs/units-annotation.md](docs/units-annotation.md) | Drawing and display units, plot scale, annotation sizing and layout |
| [docs/workspaces.md](docs/workspaces.md) | Pro and Classic workspaces, panels, text editing, hatch selection |
| [docs/maintenance.md](docs/maintenance.md) | Flatten on import, `PURGE`, `AUDIT`, `RECOVER`, backups, DWG worker |
| [docs/ai-agent.md](docs/ai-agent.md) | Local AI agent: tools, validation, confirmation, loop limits |
| [docs/status.md](docs/status.md) | Implemented and planned functionality (capability matrix) |
| [docs/documentation-policy.md](docs/documentation-policy.md) | Documentation maintenance and change management (required for all contributors and agents) |
| [docs/hcw-kit-port.md](docs/hcw-kit-port.md) | Architectural tools ported from hcwCAD-KIT |
| [docs/parametric.md](docs/parametric.md) | Parametric graph engine, wall commit and take-off facts |
| [docs/HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md](docs/HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md) | Hand-off to MynaOps: parametric wall take-off rows |
| [docs/projects.md](docs/projects.md), [docs/sheets.md](docs/sheets.md), [docs/widgets.md](docs/widgets.md), [docs/input-forms.md](docs/input-forms.md) | Floor projects and Xrefs, layouts and plotting, calculator and Pomodoro, input forms |
| [docs/control-protocol.md](docs/control-protocol.md) | JSON control channel |
| [docs/threat-model.md](docs/threat-model.md) | Loopback trust boundary, signing secrets, supply-chain must-nots |
| [docs/mcp.md](docs/mcp.md) | MCP server |
| [docs/plot-styles.md](docs/plot-styles.md) | Plot styles and plotting |
| [docs/releasing.md](docs/releasing.md) | Versioning, release workflow, signing |
| [docs/BUG_BACKLOG.md](docs/BUG_BACKLOG.md), [docs/UI_BASELINE.md](docs/UI_BASELINE.md), [docs/UI_BUG_BACKLOG.md](docs/UI_BUG_BACKLOG.md) | Quality programme records |
| [docs/UI_TESTING_STRATEGY.md](docs/UI_TESTING_STRATEGY.md), [docs/UI_TEST_MATRIX.md](docs/UI_TEST_MATRIX.md) | UI test tooling, coverage |
| [docs/WCAG_UI_AUDIT.md](docs/WCAG_UI_AUDIT.md) | Accessibility and interface audit of the WinUI shell: method, findings, fixes |
| [docs/levels-and-sheet-sets.md](docs/levels-and-sheet-sets.md), [docs/smart-blocks.md](docs/smart-blocks.md), [docs/project-and-presentation.md](docs/project-and-presentation.md), [docs/PROJECT_WIZARD.md](docs/PROJECT_WIZARD.md) | Levels and drawing tabs, smart blocks, project details and presentation import, the Project Wizard |
| [docs/pdf-underlay.md](docs/pdf-underlay.md), [docs/revision-history.md](docs/revision-history.md) | PDF underlay and `pdf.import`, revision history |
| [docs/import.md](docs/import.md) | `import`: DXF, DWG, PDF, SVG, PNG, JPEG and TIFF |
| [docs/UI_STANDARDS.md](docs/UI_STANDARDS.md), [docs/UI_TOKENS.md](docs/UI_TOKENS.md), [docs/WINDOW_SIZING.md](docs/WINDOW_SIZING.md), [docs/TYPOGRAPHY.md](docs/TYPOGRAPHY.md), [docs/COMPACT_CAD_UI.md](docs/COMPACT_CAD_UI.md), [docs/canvas-smoothness.md](docs/canvas-smoothness.md) | WinUI shell standards, tokens, window sizes, type, compact layout, canvas rendering plan |
| [docs/qa/README.md](docs/qa/README.md) | Quality records: test matrix, defect ledger, regression and release readiness |
| [docs/licensing/DEPENDENCY_INVENTORY.md](docs/licensing/DEPENDENCY_INVENTORY.md), [docs/licensing/PROPRIETARY_COMPONENT_REMOVAL.md](docs/licensing/PROPRIETARY_COMPONENT_REMOVAL.md) | Dependency licences, proprietary components not shipped |
| [docs/architecture/suite-integration.md](docs/architecture/suite-integration.md) | How MynaCAD connects to MynaHub and MynaOps (see docs/SUITE.md) |
| [docs/parity-checklist.md](docs/parity-checklist.md) | Generated menu-name comparison (`cargo xtask parity`) |
| [CLAUDE.md](CLAUDE.md) | Rules for developers and coding agents |

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Third-party notices are in
[NOTICE](NOTICE) and asset sources in [ATTRIBUTION.md](ATTRIBUTION.md).
