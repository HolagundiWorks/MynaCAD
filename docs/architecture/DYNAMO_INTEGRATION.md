# Dynamo companion integration

Parametric wall take-off graphs use `aad-param` (`param.*`); see [parametric.md](../parametric.md).
> **Status (2026-11-11):** O-Dynamo is no longer part of the suite. This page describes the optional pipe hook that stays in MynaCAD, inactive; see [SUITE.md](../SUITE.md).
Graph evaluation for companion drafting uses a separate **O-Dynamo** host (`ODynamo.Host`) over a
local named pipe.
Enhancement roadmap: [dynamo-enhancement-plan.md](../dynamo-enhancement-plan.md).

## Pipe protocol (v2)

Named pipe: `ODraft.Dynamo` (Windows only). JSON lines, one request or push per line.

| Method | Direction | Purpose |
|---|---|---|
| `ping` | MynaCAD → Host | Liveness |
| `evaluate` | MynaCAD → Host | Evaluate the host graph (or `params.graph`) with the stored context |
| `context.levels` | MynaCAD → Host | `{ levels: [{name, elevation, height, active}] }` |
| `context.project` | MynaCAD → Host | `{ project: { title, owner, …, details, floors } }` |
| `subscribe` | MynaCAD → Host | Host pushes `evaluation` on graph edits (debounced 300 ms) |
| `unsubscribe` | MynaCAD → Host | Stop pushes |
| `drawing.changed` | MynaCAD → Host | Acknowledged; source-entity tracking is later |
| `evaluation` | Host → MynaCAD | Push: `{ lines, circles, arcs, inserts, titleFields, texts, dimensions, diagnostics, generation }` |

Evaluate / evaluation `result` includes `arcs` (radians, CCW from +X), `inserts`
(`blockName`, `x`, `y`, `rotation` degrees, `attributes`, optional `level`),
`titleFields` (`field`, `value`), `texts` / `dimensions` (optional `level` + `sheetSet`).

## Engine commands

| Command | Role |
|---|---|
| `companion.connect` | Ping, push levels + project context, mark connected |
| `companion.disconnect` | Drop subscription and connected flag |
| `companion.status` | `{ connected, subscribed, pipe, preview, available }` |
| `companion.evaluate` | One-shot evaluate; stores `preview` |
| `companion.subscribe` / `unsubscribe` | Background reader for live pushes into `preview` |
| `companion.preview` | Last stored preview; optional `{ result }` stashes (or `null` clears) for the canvas ghost |
| `companion.commit` | Write `result` (or last preview): geometry/inserts onto named levels; texts/dimensions onto sheet-set annotation; title fields onto the title block; creates missing levels; unknown blocks/sheet sets become warnings; one undo step |

Menus: Architecture > Companion. WinUI invokes these through `app.run` (no separate bridge class).

Non-Windows / wasm: pipe methods return a clear error (`status.available` is false); `companion.commit` from a supplied `result` works on every target.

## Graph context nodes (O-Dynamo)

| Node | Role |
|---|---|
| `levelSelect` | Outputs level `name`, `elevation`, `height` from the last `context.levels` |
| `projectField` | Reads a field from the last `context.project` (top-level or `details`) |
| `blockInsert` | Emits a block insert (`name`, `x`, `y`, `rotation` degrees, `attribs`, optional `level`) |
| `titleField` | Emits a title-block field tag/value (e.g. `REV`, `SCALE`) |
| `textNote` | Emits annotation text (`text`, `x`, `y`, `height`, `rotation`, optional `level`/`sheetSet`) |
| `dimLinear` | Emits a linear dimension (`x0`…`y1`, `offset`, optional `level`/`sheetSet`) |

## Verified

- O-Dynamo: `dotnet test` on `ODraft.Graph.Tests` (15 passed), including annotation nodes.
- MynaCAD: `cargo test -p aad-engine --test companion` (12 passed): level commit, inserts, title fields, Electrical annotation, preview stash → ghost entities.
- Host: `cargo test -p aad-host overlay::` includes companion ghost overlay strokes.

## Canvas ghost

`companion.preview` / evaluate / subscribe store a result on the session. The WinUI host overlay
draws lines/circles/arcs from that result in faded cyan (`_COMPANION`, Handle 0, not selectable).
Evaluate from the Graph window stashes the preview so the ghost appears before Commit.

## WinUI Graph window (Phase 6)

**Window > Graph** manager (not on the side rail): node canvas (drag, add wall/door/number,
double-click edit) and JSON mode. Evaluate uses in-process `GraphEngine` when built with sibling
O-Dynamo (`ODRAFT_GRAPH`), else `companion.evaluate`. Commit via `companion.commit`.
