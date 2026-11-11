---
paths:
  - "crates/aad-host/**"
  - "crates/ui-egui/**"
  - "crates/mcp/**"
  - "apps/aad-winui/**"
  - "docs/control-protocol.md"
---

## Running and inspecting the app
- `cargo run --release -p aad -- --sample --control PORT` starts the app with a sample drawing and the control channel. Pick a free port.
- On Windows, the default `aad` binary (no eframe) starts the WinUI shell when `ODraft.WinUi.exe` is beside it or under `target/aad-winui` (or set `AAD_WINUI`), forwarding `--sample`, `--control` and drawing paths. Optional egui: `cargo build -p aad --features egui` then `aad --egui`. The status bar shows the loopback port (`--control` / `AAD_CONTROL_PORT`) for `engine.execute`, `cmdline.*`, `drawing.inspect` and `app.open` / `app.save` / `app.quit`. Build it with `cargo build -p aad-host`, then `dotnet build apps/aad-winui/Aad.WinUi.csproj -c Debug -p:Platform=x64 -o target/aad-winui` (copy `aad_host.dll` beside the exe). Open/Save/Save As and control `app.open`/`app.save` use workers (`SaveFileAsync` on the shell). Dual input channels: [docs/COMMAND_BAR.md](../../docs/COMMAND_BAR.md). Interactive pan/zoom path: [docs/PERFORMANCE.md](../../docs/PERFORMANCE.md). Window templates and shared components (every window goes through `StandardWindow.Present`; `cargo xtask ui` enforces it): [docs/UI_STANDARDS.md](../../docs/UI_STANDARDS.md). Tokens/sizing: [docs/UI_TOKENS.md](../../docs/UI_TOKENS.md), [docs/WINDOW_SIZING.md](../../docs/WINDOW_SIZING.md). `--egui` keeps the egui window (assistant UI and egui-only `ui.*` control methods).
- Drive it with JSON lines on `127.0.0.1:PORT`:
  - `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}` types at the command line like a user.
  - `{"id":2,"method":"engine.execute","params":{"command":"line","params":{"points":[[0,0],[10,0]]}}}` is the JSON form.
  - `{"id":3,"method":"ui.screenshot","params":{"path":"shot.png"}}` writes a PNG.
  - Methods: host control in `crates/aad-host` (WinUI) / `crates/ui-egui/src/control.rs` (egui); protocol: [docs/control-protocol.md](../../docs/control-protocol.md).
- For UI work, look at the result (screenshot). If no frame is presented (screen locked) use `ui.render` or `aad-cli`.
- Headless: `aad-cli run --sample --script 'LINE 0,0 10,10\n' --export out.png`; `aad-cli info file.dxf`; `aad-cli mcp`.
- Parallel agents: use a separate `CARGO_TARGET_DIR` per agent and edit only the crates you own.
