# UI testing strategy

How the MynaCAD interface is tested, which tools were chosen and why, how to run it, and what is not
covered yet. Windows is the supported platform for now.

## What the interface is (inspected, not assumed)

| Question | Answer |
|---|---|
| Framework | The tests below cover the egui window (`cargo build -p aad --features egui` then `aad --egui`): native Rust, **egui / eframe 0.36** (`crates/ui-egui`); a thin layer over the engine (`app.run`, `app.start`, `app.cmdline`). The default Windows binary is WinUI-only; that shell is covered by `aad-host` tests |
| Rendering | The drawing is a GPU mesh painted through an `egui-wgpu` callback (`crates/ui-egui/src/gpu.rs`); widgets are egui's own. When there is no GPU target the canvas falls back to painting on the CPU |
| Input | egui events: pointer, keys, wheel, text. The canvas reads them in `canvas.rs`; shortcuts in `menus.rs::shortcuts` |
| Tools, panels, dialogs | Tools are engine commands with prompts; panels are docked (`docking.rs`); dialogs are `formdialog.rs` (generated from the command forms) and `dialogs.rs` |
| Stable identifiers | egui does not expose an OS accessibility tree here, but AccessKit labels are present: the control channel's `ui.elements` lists 66 elements at launch, every one with a label |
| Test mode | `CadApp` runs without a window under `egui_kittest`; the real app has a JSON control channel (`--control PORT`, `docs/control-protocol.md`) with `ui.inspect`, `ui.elements`, `ui.click`, `ui.key`, `ui.screenshot` |
| Platforms | Windows x64 now; macOS, Linux and web later |

## Tools chosen

| Need | Tool | Why |
|---|---|---|
| Interaction tests with real pointer and key events | **`egui_kittest`** (already a dev-dependency, 0.36) | It runs the actual `CadApp::logic` and `CadApp::ui` and injects egui events at screen positions, so tests click where a person clicks, inside the real layout. No second framework needed |
| Application state | the engine's `Session` (selection, running command, drawing) | Assertions check geometry and command state, not only that an event fired |
| Whole-menu checks | `menus::tree` + `CadApp::start` | Every menu entry is walked automatically |
| Visual checks | `Harness::render()` (wgpu) writing PNGs, in existing tests, to a folder named by `CRAFT_UI_VISUAL_DIR` | Already used by `chrome_geometry` and `docking_workflows` |
| Real-application measurement | the control channel against `aad.exe` | The only way to measure the GPU canvas; the harness cannot |
| Browser tools (Playwright etc.) | **not used** | The product is a native application; the web build is a later phase |
| OS-level UI automation | **not used** | `egui_kittest` plus the control channel cover the same ground with stable results |

No overlapping framework was added. One dev-dependency was added to `aad-ui-egui`
(`aad-io`, a workspace crate, to register file formats in the save-and-reopen test).

## The suites

| File | What it does | Kind |
|---|---|---|
| `crates/ui-egui/tests/drawing_workflow.rs` | Draw a line, circle, rectangle and polyline by clicking; Escape and Enter; click, window and crossing selection; move, copy, rotate, erase; undo and redo; zoom extents; resize the window at four sizes and pick an object at each; save and reopen a DXF | deterministic, runs in `cargo test` |
| `crates/ui-egui/tests/menu_integrity.rs` | Walks all 282 menu entries: each command id exists; activating each one has an observable effect, except nine that are listed with a reason | deterministic |
| `crates/ui-egui/tests/ui_latency.rs` | Times frames, pointer moves, tool activation, click select, property update and wheel zoom at 10 000 and 100 000 entities | **hardware dependent, `#[ignore]`**, prints numbers, asserts nothing about speed |
| existing: `chrome_geometry`, `docking_workflows`, `shared_controls`, `workspace_modes`, `workspace_focus`, `context_menu`, `dialog_dispatch`, `style_dialogs`, `text_double_click`, `system_cursor`, `widgets`, `control_cmdline*`, `open_bytes` | status-bar overlap at four window sizes and two scales, docking drag/float/close/reload, control sizing, Pro/Classic workspaces, focus, context menu, dialogs, text edit, widgets | deterministic |

## Run locally

```text
cargo test -p aad-ui-egui                                   # all UI tests (about 15 s after a build)
cargo test -p aad-ui-egui --test drawing_workflow           # the drawing workflow
cargo test -p aad-ui-egui --test menu_integrity -- --nocapture   # prints the menu summary
CRAFT_UI_VISUAL_DIR=target/ui-shots cargo test -p aad-ui-egui --test chrome_geometry   # save screenshots

cargo test --release -p aad-ui-egui --test ui_latency -- --ignored --nocapture         # frame timing
AAD_LATENCY_N=10000 cargo test --release -p aad-ui-egui --test ui_latency -- --ignored --nocapture
```

To measure the real GPU canvas, write a drawing and open it in the app:

```text
AAD_LATENCY_WRITE=%TEMP%\synth.dxf AAD_LATENCY_COUNT=100000 cargo test --release -p aad-ui-egui --test ui_latency write_synthetic -- --ignored
target\release\aad.exe --control 47123 %TEMP%\synth.dxf
```

then send `ui.inspect` (it returns `perf.frameMs`, `buildMs`, `meshMs`, `renderer`) and `ui.move` over
`127.0.0.1:47123` as described in `docs/control-protocol.md`.

## Writing a UI test

- Build the app like the existing tests (`CadApp::new`, `app.ui.in_window_menu = false`, a `Harness`
  sized explicitly, `run_steps(5)` to settle).
- Convert world to screen with `app.canvas.xf` and click there; **keep gestures away from the
  canvas edge**, where the docked panel's resize area takes the press. (Two of the first draft tests
  failed for exactly this reason and one for a wrong expectation; neither was an application bug.)
- A drag needs several intermediate `PointerMoved` events, as `drag` in the test does.
- Assert the drawing, selection and `session.running`, not only that an event was sent.
- Before trusting a new test, make it fail once (wrong expectation or removed fix).

## Interaction model (what the tests encode)

This product follows the classic click model; the tests state it so a change is noticed:

- a click on empty space opens a selection window (it does **not** deselect); Escape clears the
  selection and cancels the window;
- a drag opens a window: left to right selects what is fully inside, right to left also selects what
  it crosses;
- Escape cancels a command, Enter finishes a polyline, right-click acts as Enter while a command
  runs.

## Not covered yet

See [UI_TEST_MATRIX.md](UI_TEST_MATRIX.md) for the item-by-item status. In short: no approved
screenshot baselines, no automated check of dialogs staying inside small windows beyond the existing
ones, no per-control keyboard (Tab order) test, no real-GPU frame measurement in CI, the UI tests
are not yet a separate CI job. The harness paints on the CPU, so its frame times are not the GPU
app's.
