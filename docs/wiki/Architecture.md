# Architecture

The engine is a set of Rust crates in strict layers. `cargo xtask layers` enforces them; the table in `xtask/src/layers.rs` is the authority.

| Layer | Crates |
|---|---|
| L0 | `geom`, `color`, and standalone `dxf`, `dwg`, `param`, `odp`, `llm` |
| L1 | `doc` |
| L2 | `geodb`, `fonts`, `render` |
| L3 | `io` |
| L4 | `engine` |
| L5 | `ui-egui`, `host`, `mcp`, `agent`, `auditor`, `vello` |
| apps | `aad` (and `aad-cli`, `aad-web`), `apps/aad-winui` (C# shell) |

Nothing below L5 depends on a UI toolkit or GPU library. `aad-host` (a C ABI DLL, `aad_host.dll`) runs a session and paints the shared canvas mesh into a Win32 child window; the C# shell in `apps/aad-winui` calls only that ABI.

## Everything is a command
User-visible behaviour is a command in `crates/engine/src/cmd/*` with an id, label, menu path, parameters, an `enabled` rule and a JSON `run`. The command line, menus, toolbars, scripts, the CLI, the control channel, the MCP server and the AI agent tools all reach the same commands. Programmatic calls never open dialogs.

## Never crash
No `unwrap`, `expect`, `panic!` or `unsafe` in non-test code (the host and the DWG worker job object are the documented exceptions). Input from files, the command line, MCP and the control channel is treated as hostile. `Session::execute` runs under `catch_unwind` as a last resort.

## Files
A foreign DWG or DXF is never edited in place: it opens as a native `.osp` working copy with autosave and backups; DWG/DXF output is an explicit export. The `.osp` is shared with MynaOps.

## The AI agent
MynaCAD only proposes calls to allow-listed tools; geometry, repairs and quantities always come from deterministic engine commands. MynaOps hosts the model.
