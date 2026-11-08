# Local AI agent

A small offline language model (Qwen3-1.7B) can interpret a request such as "audit and repair this
drawing" and propose a call to one of a fixed set of CAD tools. The model never draws, repairs or
calculates. It only chooses a tool and fills in its arguments; deterministic engine commands do the
work.

## Status

| Part | State |
|---|---|
| Tool registry, strict parsing, schema validation, confirmation policy, loop limits, audit log (`crates/agent`) | **Implemented and tested** (13 unit tests) |
| Tools: `get_drawing_summary`, spatial query tools, `flatten_drawing`, `audit_drawing`, `purge_unused`, `recover_file`, units/plot/annotation layout tools | **Implemented and tested**, each one an existing engine command |
| Auditor v0 detect tools: `validate_drawing`, `check_command_parity`, `check_command_menu_coverage`, `check_ui_tokens`, `auditor_report` (plus detect path of `audit_drawing`) | **Implemented and tested** via `aad-auditor`; detect-only (no auto repair). Repo checks need the source tree (`AAD_REPO_ROOT` or cwd). Screenshot capture stays on the live Windows control/MCP path |
| Qwen3 chat format, model-file discovery and checks, generation settings (`crates/llm`) | **Implemented and tested** (7 tests, no model needed) |
| Assistant conversation state machine: tool list prompt, strict parse, one retry, confirmation, step and history limits (`agent::Assistant`) | **Implemented and tested** with scripted model replies (8 tests) |
| llama.cpp runtime (`aad-llm` with feature `llama`) | **Builds and runs** (2026-10-12, LLVM 23.1.3 from `winget install LLVM.LLVM`, `LIBCLANG_PATH=C:\Program Files\LLVM\bin`, CMake, MSVC). Tests and `clippy -D warnings` pass with the feature. The model loads in about 1 s (memory-mapped) and generates about 17 tokens per second on this machine's CPU (no GPU backend) |
| Model on this machine | `Qwen3-1.7B-Q4_K_M.gguf`, 1 107 409 472 bytes, from `unsloth/Qwen3-1.7B-GGUF`, SHA-256 `b139949c...a181897` (equals the hash Hugging Face publishes), in `%APPDATA%\Tren\models`. Not in the repository |
| Background worker (`aad_llm::worker`): loads on the first request, one request at a time, Stop, panic contained | **Implemented**; a unit test covers the no-model path |
| `driver` (`aad_agent::driver`): runs the Assistant against a model for hosts that can wait | **Implemented** |
| `aad-cli ask [--file f \| --sample] [--yes] [--save out.dxf] "request"` (build with `--features assistant`) | **Implemented, run with the real model** (below) |
| Assistant through the WinUI fixed AI prompt and optional egui unified bar (see [COMMAND_BAR.md](COMMAND_BAR.md)): thinking indicator, Allow/Decline, Escape to stop; model on the worker thread, tools through the engine on the UI thread | **Implemented** for WinUI (`aad-host --features assistant`, `aad_host_ask`, Allow/Decline on the AI bar) and for optional egui (`aad --features egui,assistant`). egui bar behaviour covered by `crates/ui-egui/tests/command_bar.rs` with a fake backend; host unit test covers the no-runtime path |
| Bundling the model in the installer, and building the installer with the `assistant` feature | **Implemented:** `packaging/windows/package.ps1 -Arch x64 -Assistant` builds `o-draft-ai-<version>-windows-x64.msi` (and a portable zip) with `aad` / `aad-cli` / `aad-host` linked for the assistant, the model under `<install>\models\`, open-source EULA page, and installer splash bitmaps. Needs the model at `%APPDATA%\Tren\models\Qwen3-1.7B-Q4_K_M.gguf` or `AAD_MODEL_FILE`, plus CMake, a C++ toolchain and LLVM libclang |
| Result tables, entity highlighting | **Planned** |
| Retrieval of project notes and office standards (RAG) | **Planned** |
| Read tools for take-off, rooms, drawing revision and permit layers | **Implemented and tested:** `get_takeoff_result` (`takeoff.list`), `get_room_log`, `get_room_area_total`, `get_drawing_revision`, `check_permit_layers`; all read-only, no file path in any schema |
| Schedule of openings, running a take-off | **Planned.** The engine commands exist (`schedule`, `openings.table`, `takeoff.*`, see [hcw-kit-port.md](hcw-kit-port.md)) but they draw into the drawing and are not yet in the agent registry. The names are reserved in `agent::PLANNED` and any call to them is refused as "not implemented". |

Requests such as "Prepare a schedule of openings" and "Run the built-in quantity takeoff" therefore
cannot be carried out by the agent today, although the commands can be run directly. The agent must say so rather than answer.

## Supported requests

| Request | Tool | Confirmation |
|---|---|---|
| "Flatten the imported drawing." | `flatten_drawing` | yes (changes the drawing, one undo step) |
| "Audit this drawing." | `audit_drawing` `{}` | no (report only) |
| "Audit and repair this drawing." | `audit_drawing` `{"fix": true}` | yes |
| "Validate this drawing." | `validate_drawing` `{}` | no (detect only) |
| "Run the auditor report." | `auditor_report` | no (detect only; never repairs) |
| "Check command/menu parity." | `check_command_parity` / `check_command_menu_coverage` | no |
| "Check WinUI tokens." | `check_ui_tokens` | no |
| "Purge unused definitions." | `purge_unused` | yes (removes definitions, one undo step) |
| "Recover this damaged file." | `recover_file` `{"path": ...}` | no (opens a repaired copy; the file is not changed) |
| "What is in this drawing?" | `get_drawing_summary` | no |

## Tool schemas

Offered to the model in the function-calling format of its chat template
(`agent::function_schemas()`). Arguments are checked against these schemas with
`additionalProperties: false`; an unknown or mistyped argument rejects the call.

```json
{"name": "get_drawing_summary", "parameters": {"type": "object", "properties": {}, "additionalProperties": false}}
{"name": "flatten_drawing",     "parameters": {"type": "object", "properties": {}, "additionalProperties": false}}
{"name": "audit_drawing",       "parameters": {"type": "object", "properties": {"fix": {"type": "boolean"}}, "additionalProperties": false}}
{"name": "purge_unused",        "parameters": {"type": "object", "properties": {
    "type": {"type": "string", "enum": ["all","blocks","dimstyles","groups","layers","linetypes","mleaderstyles","tablestyles","textstyles","zerolength","emptytext"]},
    "names": {"type": "string", "maxLength": 256}}, "additionalProperties": false}}
{"name": "recover_file",        "parameters": {"type": "object", "properties": {
    "path": {"type": "string", "minLength": 1, "maxLength": 1024}}, "required": ["path"], "additionalProperties": false}}
```

Results are structured JSON (`{"tool", "status", "data_is_untrusted": true, "data"}`), never prose
to be parsed. `data` can contain text from the drawing, which is why it is marked untrusted.

## Execution sequence

1. The application resolves the open drawing and any selection itself; the model does not guess.
2. It sends the system instruction (`agent::SYSTEM_PROMPT`), the user request and the tool schemas.
3. The reply is parsed by `parse_tool_call`: exactly one `<tool_call>{"name","arguments"}</tool_call>`
   block (Qwen's format) or a bare JSON object. Prose that mentions a tool, several calls in one
   reply, extra keys, malformed JSON and oversized replies are all refused.
4. `Agent::propose` refuses an unknown tool, a planned (unbuilt) tool, arguments that fail the
   schema, more than `max_calls` (3) calls per user request, and an identical repeat of the
   previous call.
5. A call that changes the drawing returns `NeedsConfirmation` with a one-line summary. The
   application shows it and, only if the user agrees to exactly that call, proposes it again with
   `confirmed = true`. Nothing runs before then.
6. The engine command runs; the result (or error) goes back to the model, which writes a short
   summary from it. Tables are rendered from the structured result by the application.
7. Every proposal, including refusals, is appended to the audit log with the request number, tool,
   arguments, status and elapsed time. Drawing contents are not logged beyond the arguments.

## Permissions

| Risk | Meaning | Confirmation |
|---|---|---|
| `Read` | reads the drawing | no |
| `Open` | opens another drawing; nothing existing changes | no |
| `Mutating` | changes the open drawing | yes |
| `Destructive` | removes definitions from the open drawing | yes |

The model cannot run shell commands, scripts, SQL, file access or network requests; the registry is
the whole of its reach. Text found in drawings, imported files and tool results is data and cannot
create a call, because calls are only taken from the model's own reply.

## Error handling

| Situation | Result |
|---|---|
| Unparseable or unknown call | `Rejected`, nothing runs, logged |
| Planned tool | `Rejected: ... is not implemented yet` |
| Invalid arguments | `Rejected` with the offending argument named |
| Over the call limit or repeated call | `Rejected`, the loop stops |
| Tool error (for example a file that cannot be read) | `Failed` with the engine's message |

## Model runtime (planned)

Values to benchmark when the adapter is built, from the Qwen3 model card and tuned for tool routing:
Qwen3-1.7B (Apache-2.0, pin an exact revision and file hash), llama.cpp with a local GGUF
(start with Q4_K_M), non-thinking mode (`enable_thinking=false`, verify it is enforced by the
pinned build's chat template), temperature 0.2, top_p 0.8, top_k 20, min_p 0, 8192 tokens of
context, at most 512 new tokens for tool routing and 1200 for a summary. These are in
`agent::GenerationProfile`. The runtime must stay local: loopback only, no external AI API, no web
dependency after installation. The adapter must be replaceable without changing the tool API.

## Tests

`cargo test -p aad-agent` covers: parsing (valid blocks, prose, malformed JSON, extra keys,
multiple calls, oversize), schema validation, that only built tools are offered, refusal of unknown
and planned tools, confirmation before changes and one undo step after, audit with and without
`fix`, detect-only `validate_drawing` and `auditor_report`, purge, argument rejection, the repeat
and call limits, text in a drawing never becoming a call, `recover_file` leaving the file
untouched, and the audit log. `cargo test -p aad-auditor` covers the detect wrappers and the
combined JSON report.

## Auditor v0 (detect-only)

Aligned with the rule that the model never calculates or repairs: Auditor v0 only **proposes**
allow-listed detect tools. Deterministic code in `crates/auditor` returns structured findings.
Fixes remain human- or command-driven (`audit` / `validate` with `fix: true` only after
confirmation, as today).

| Tool | Backing | Confirmation |
|---|---|---|
| `audit_drawing` `{fix?: false}` | engine `audit` | no when detect; yes when `fix: true` |
| `validate_drawing` `{fix?: false}` | engine `validate` | no when detect; yes when `fix: true` |
| `check_command_parity` / `check_command_menu_coverage` | menu catalog vs `command_specs` (same breadth as `cargo xtask parity`; does not rewrite `docs/parity-checklist.md`) | no |
| `check_ui_tokens` | WinUI scan (same rules as `cargo xtask ui`) | no |
| `auditor_report` `{checks?}` | combined JSON (`audit`, `validate`, `parity`, `ui`) | no |

CLI entrypoint: `aad-cli auditor [--file FILE \| --sample] [--checks LIST] [--out REPORT.json]`.
Screenshot capture is **not** part of the headless report; the report's `screenshotCapture` field
documents the Windows hook (`ui.screenshot` / MCP `screenshot` with `--connect`). Out of scope for
v0: Phase B tiling, TLS, screenshot golden CI on Linux.

## Units and layout requests

| Request | Tool | Confirmation |
|---|---|---|
| "Set drawing units to millimetres and display dimensions in metres." | `set_drawing_units` `{units: mm, mode: metadata}` then `set_display_units` `{units: m}` | yes |
| "Rescale the drawing to metres." | `set_drawing_units` `{units: m, mode: scale}` | yes, destructive |
| "Prepare this drawing for printing at 1:100 on A1." | `set_plot_configuration` then `validate_plot_layout` | yes (configuration) |
| "Find all text overlapping walls or dimension lines." | `analyze_annotation_collisions` | no |
| "Preview and correct all annotation conflicts." / "Resolve annotation overlaps without moving the walls." | `preview_layout_changes`, then `apply_layout_changes` | yes; destructive with `includeManual` |

"Automatically adjust all dimension text and arrows" is `set_plot_configuration`. "Arrange the
opening schedule with consistent spacing" cannot be done: there is no opening schedule feature. The
model never chooses `mode` for it: the schema requires it, and a request that does not say whether
to rescale is answered with a question. Details: [units-annotation.md](units-annotation.md).

## What was run with the real model (2026-10-12, one machine, CPU, Qwen3-1.7B Q4_K_M)

Observed, not benchmarked: one run each, so these are examples, not rates.

| Request | What the model did | Result |
|---|---|---|
| "What is in this drawing?" | called `get_drawing_summary` | the tool ran; the summary text was mostly right but added things that were not in the tool result ("a complex polyline structure", "the drawing is clean and has no errors") and counted a circle twice. About 30 s |
| "Audit and repair this drawing." | proposed `audit_drawing` with repair | asked for confirmation; answered **no**; nothing ran, the model reported it had been declined |
| "Purge unused definitions." with `--yes` | proposed `purge_unused` | confirmation shown, allowed; 17 linetypes were removed through the engine; the model's summary listed them. About 24 s |
| "Prepare a schedule of openings." | no tool call | refused as unsupported, nothing ran. About 8 s |
| "Audit this drawing." (desktop window) | proposed `audit_drawing` **with repair** | a read-only request got a changing proposal; the confirmation step is what stopped it. Whether the model does this consistently was not measured |

Conclusions that hold: tool selection and refusal behaved sensibly on these five requests; the
confirmation gate and the "model never changes the drawing itself" rule work. Conclusions that do
not hold yet: reliability across many requests (not measured), and the summaries cannot be trusted
for details, so the application should show tool results (tables) rather than rely on the model's
retelling of them (planned).

## Asking from the UI

**WinUI:** type in the fixed AI prompt, or type `/…` on the idle CAD prompt to transfer the text
(without the leading slash) into that AI field, then Enter. Calls `aad_host_ask` — not CAD
`cmdline`. Plain sentences typed on the CAD prompt are CAD (Space/Enter run commands); they are
not routed to the assistant. Rules: [COMMAND_BAR.md](COMMAND_BAR.md).

**egui (optional):** unified bar still uses a leading `/` or `/reference` for asks
(`crates/ui-egui/tests/command_bar.rs`).

## Spatial knowledge

The model has no sense of space on its own: a drawing is lists of numbers. It is given the layout
by read-only, bounded engine queries (`crates/engine/src/cmd/spatialq.rs`, tested in
`crates/engine/tests/spatial_query.rs`), exposed as tools that need no confirmation:

| Tool | Engine command | What it answers |
|---|---|---|
| `get_spatial_overview {cols?, rows?}` | `spatial.overview` | Extents, the part on screen, and a grid (row 0 at the bottom, column 0 at the left) of how many objects of which types and layers lie where |
| `find_objects_in_window {window, mode?, layers?, types?, limit?}` | `spatial.query` | The objects touching or inside a rectangle: handle, type, layer, bounds, centre and key geometry; `matched` and `truncated` say when the list was cut |
| `find_nearest_objects {point, count?, layers?, types?}` | `spatial.nearest` | The objects closest to a point, with the distance to their bounds |
| `describe_objects {handles}` | `spatial.describe` | Details by handle: a line's ends and length, a circle's centre and radius, a text's words and position, a block's name |

The references add positions too: `/view` (the visible window and the objects in it), `/selection`
(each object's bounds), `/rooms` (each label's position) and `/drawing` (extents and view).
Coordinates are drawing units with Y up, in the space being edited. Positions come from object
bounds, so a block reference is located by what it draws. Every result has a size limit, nothing
here changes the drawing, and the model still never calculates: distances and counts come from the
tools. What it cannot do: it does not see the picture, and it does not know what a room *is* beyond
the labels the ROOM command made.
