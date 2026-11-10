# MynaCAD and AI agents (MCP)

MynaCAD speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

## Start it

```sh
# Headless: an in-process drawing session, no window.
aad-cli mcp

# Bridged to the running app (watch the agent draw):
aad --control 7979 &
aad-cli mcp --connect 127.0.0.1:7979
```

`--connect` needs the app's `HOST:PORT`. Without one (or with an unknown argument) `aad-cli mcp`
exits with an error instead of falling back to a headless session, so a client meant for the app
never edits a separate drawing by mistake. If the app can't be reached, startup fails too.

The app's control port is loopback-only and unauthenticated; bridging MCP to it trusts every local
process that can open that port. Prefer stdio MCP when possible. See [threat-model.md](threat-model.md).

Example client configuration (Claude Code / Claude Desktop):

```json
{ "mcpServers": { "aad": { "command": "aad-cli", "args": ["mcp"] } } }
```

## Tools

| Tool | What it does |
|---|---|
| `command_line {text}` | Type at the command line exactly like a person: `line 0,0 @10,0 @0,5 c`, `circle 5,2 1`, `offset 0.5`, `zoom e`. Spaces act as Enter; empty text is Enter. Returns the new prompt, keywords and output. |
| `script {text}` | A multi-line script (like `.scr` files). |
| `execute {command, params}` | Any command with JSON parameters, never a dialog. `list_commands` shows ids and parameter docs. |
| `list_commands {filter?}` | The command catalog; `filter` matches part of a command's id, label or an alias. |
| `inspect_drawing {entities?, limit?}` | Counts, extents, layers, styles, blocks, layouts, selection, undo history and entities. |
| `query_entities {type?, layer?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `render {width?, height?, fit?}` | A PNG image of the drawing. |
| `new_drawing {metric?}`, `open {path}`, `save {path?}`, `cancel` | Files and Escape. `save` to a `.svg`, `.png` or `.pdf` path exports: the drawing keeps its name and unsaved changes. |
| `screenshot`, `ui_inspect`, `ui_click {x, y}` | Only when connected to the app. `screenshot` is the Auditor v0 capture hook on a live Windows shell. |
| `audit_drawing` | Auditor v0 detect-only: engine `audit` with `fix: false` (never repairs). |
| `validate_drawing` | Auditor v0 detect-only: engine `validate` with `fix: false`. |
| `check_command_parity` | Auditor v0: menu catalog vs registered commands (needs MynaCAD source tree / `AAD_REPO_ROOT`). |
| `check_ui_tokens` | Auditor v0: WinUI token/template scan (same rules as `cargo xtask ui`; needs source tree). |
| `auditor_report {checks?}` | Auditor v0: one JSON detect report. `checks` is a comma list `audit,validate,parity,ui` (default all). Screenshot capture is documented in the report only; use `screenshot` / `ui_inspect` when connected. |

Headless CLI report (no MCP): `aad-cli auditor [--file FILE | --sample] [--checks LIST] [--out REPORT.json]`.

Resources: `aad://drawing` (inspect JSON) and `aad://commands` (catalog).

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
