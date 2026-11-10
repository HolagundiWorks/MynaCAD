# Parametric graphs and the wall take-off slice

What exists for Grasshopper-style parametric modelling in MynaCAD. This is not CAD parametric
constraints (those remain out of scope). The graph computes measured wall facts; MynaCAD owns the
drawing and undo; MynaOps owns finishes, net areas and costing.

## Ownership

| Piece | Owner |
|---|---|
| Component type schemas (`osas.wall` v1) and node library | Code in `aad-param` (hosted library not built yet) |
| Graph evaluation, units, formulas, cycle detection, cache | `crates/param` (`aad-param`) |
| Drawing geometry, project data, undo, commit | MynaCAD engine (`param.*` in `crates/engine/src/cmd/param.rs`) |
| Quantity take-off and costing from measured facts | MynaOps (via `bridge.export` / `takeoff.record`) |

## Crate: `aad-param`

Standalone L0 crate (no workspace crate dependencies). It only computes: it reads no drawing and
writes nothing.

- **Graph format** version 1 (`schemaVersion`), camelCase JSON, `deny_unknown_fields`. Stored
  graphs are untrusted: structure limits, id shape, and older format 0 migration
  (`node.position` → `editor.positions`). Newer versions are refused.
- **Nodes** (v1 library): `param.number`, `param.toggle`, `param.choice`, `math.formula`,
  `geom.path`, `component.wall`, `measure.wall`.
- **Units**: dimensional arithmetic (`Dim`), SI storage, display units (mm, m, …).
- **Formulas**: `+ - * / ^`, `min`/`max`/`abs`/`sqrt`/`round`/`floor`/`ceil`, optional unit after a
  number; bounded expression length and depth.
- **Evaluation**: port type and unit checks, one edge per input, Kahn topological order, cycle path
  in diagnostics, incremental cache keyed by type@version, config and inputs; cancellable.
- **Wall component** `osas.wall` / schema 1: path, height (m), thickness (mm), material and finish
  enums; measurement rule id `hcw-aqc-bridge/1#walls`. Precedence: schema default → project level
  height → config override → graph input; a locked parameter keeps its value and warns on conflict.
- **Template** `template::add_wall`: path, height slider, thickness box, wall component, measure
  node, connected.

Limits: 5000 nodes, 20000 edges, 1000 groups, 64-character ids, 10000 path points. Tests:
`cargo test -p aad-param` (16 tests).

## Engine commands

Registered under Architecture > Parametric. JSON form only for programmatic calls (no dialogs).

| Command | Undo | Role |
|---|---|---|
| `param.library` | no | Node library and component types |
| `param.graph` | yes | Read, replace or remove the stored graph (`{graph}`, `{remove: true}`) |
| `param.wall.new` | yes | Add a wall from `handle` (line/polyline) or `points` (drawing units); optional `height`, `thickness`, `id` |
| `param.set` | yes (unless `preview`) | Set a node's `value` or merge `config`; `preview: true` evaluates without storing |
| `param.evaluate` | no | Refresh paths from source entities, evaluate, return staged face lines and diagnostics |
| `param.commit` | yes | Write staged walls as `A-WALL` face lines; optional `graphRevision` / `drawingRevision` / `force` |
| `param.status` | no | Per instance: `linked`, `stale` or `detached`, with reasons |
| `param.detach` | yes | Keep lines as plain geometry; later commits leave them alone |
| `param.deps` | no | Upstream and downstream node ids |
| `param.takeoff` | no | Measured-fact wall rows plus provenance (no finishes, rates or costs) |

### Persistence

The graph and links live in drawing project data under key `parametric`
(`{ graph, links }`), so they survive DXF save (`AAD_PROJECT` extension data) and ride the drawing
undo stack with every stored edit. The session holds a disposable `param_cache`
(`aad_param::Cache`); it is never saved.

Each committed wall link records: state, face-line handles, fingerprint, graph revision, input
hash, drawing revision, and the measured facts (`length_mm`, `height_mm`, `thickness_mm`, level,
material, finish, component type).

### Commit behaviour

1. Refresh `geom.path` nodes that follow a drawing entity (handle + fingerprint).
2. Evaluate; refuse if there is any error or the evaluation did not finish (drawing restored).
3. Refuse if optional `graphRevision` / `drawingRevision` do not match (caller must re-evaluate).
4. Refuse if linked lines were hand-edited since the last commit, unless `force`.
5. Generate face lines with the same mitred `offset_path` as `wall.draw`; reuse handles in place
   when the segment count is unchanged; otherwise replace.
6. Walls the graph no longer makes are marked detached (lines kept).

### Take-off hand-off

`param.takeoff` returns MynaOps wall rows:

```json
{ "ref": "PW-<instanceId>", "level", "length_mm", "height_mm", "thickness_mm", "layer": "A-WALL" }
```

plus provenance (`instanceId`, `componentType`, `schemaVersion`, material/finish, measurement rule,
revisions, handles). Stale and detached walls are omitted with warnings.

`bridge.export` (and therefore `takeoff.record`) includes linked, current parametric walls alongside
brick take-off walls. Parametric rows carry `thickness_mm`; brick take-off walls still do not.
Gross face area and volume from `measure.wall` are graph measurements for preview only — not the
BOQ. Finishes and net areas stay with MynaOps. Hand-off for MynaOps (row shapes, gaps, questions):
[HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md](HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md).

## Tests

`crates/engine/tests/param.rs` (5 tests): preview/commit/undo/redo and bridge row; stale after
value or source change and in-place update; hand-edited lines refused unless `force`, detach;
cycle and hostile params change nothing; graph survives DXF round-trip and evaluates after reload.

## O-Dynamo companion (separate process)

A Windows named-pipe companion evaluates a small 2D drafting graph (doors, windows, columns,
stairs, grid, `levelSelect` / `projectField`, `blockInsert`, `titleField`, `textNote`,
`dimLinear`). Engine commands: `companion.connect`, `disconnect`, `status`, `evaluate`,
`subscribe`, `unsubscribe`, `preview`, `commit` (geometry on levels, inserts, title fields,
sheet-set annotation). Protocol and status:
[architecture/DYNAMO_INTEGRATION.md](architecture/DYNAMO_INTEGRATION.md). Roadmap:
[dynamo-enhancement-plan.md](dynamo-enhancement-plan.md). This does not replace `param.*` wall
take-off.

## Not done

- WinUI (or egui) node-graph editor for `param.*` walls (companion Graph window is separate; see
  [architecture/DYNAMO_INTEGRATION.md](architecture/DYNAMO_INTEGRATION.md)).
- Async C ABI for long evaluations.
- Hosted component library (schemas are in code).
- A MynaOps `measure(component, rule, revision)` API (the published contract is measured-fact rows
  through the bridge / `takeoff.record`; derived finishes must not be sent from MynaCAD).
- Columns, beams, slabs and other component types beyond `osas.wall` in `aad-param` (companion
  has draft geometry, block-insert and sheet-set annotation nodes).
- Agent tools for `param.*` (engine commands exist; the agent registry does not expose them yet).
