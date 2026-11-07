# Dynamo companion enhancement plan

**Status: complete for Phases 1–6 scope in this plan.** Verified: O-Dynamo `dotnet test` 15;
`cargo test -p aad-engine --test companion` 12; `aad-host` overlay ghost test. Phase 2 ghost
overlay and Phase 6 visual node canvas are implemented. Capability matrix: [status.md](status.md).

## Goal

Make the companion graph engine useful for real architectural plans by connecting it to
Tren's new features (levels, sheet sets, title plate, smart blocks, project details,
WinUI design system) while keeping both repos independently buildable and the pipe protocol
versionable.

---

## Inventory: what exists today

### O-Dynamo companion

| Project | What it does |
|---|---|
| `ODraft.Geometry` | `Vec2`, `Line2`, `Circle2`, `Box2`, `Draft2D` (Rectangle, WallOutline, OffsetOpen, PolygonArea, Bounds) |
| `ODraft.Units` | `Lengths` (mm ↔ m/cm/in/ft) |
| `ODraft.Graph` | `GraphEngine.Evaluate` with drafting + context + insert/title/annotation nodes, `GraphContext`, topological sort, diagnostics, geometry + `inserts`/`titleFields`/`texts`/`dimensions` |
| `ODynamo.Host` | WPF window (JSON editor + Run), pipe v2 (`evaluate`, `ping`, `context.*`, `subscribe`), named pipe `ODraft.Dynamo` |

### Tren engine commands that generate the same geometry

| Engine command | What it draws | Layer |
|---|---|---|
| `wall.draw` | Centre-line walls with offset faces | `A-WALL` |
| `wall.door` | Door opening (jamb gap + swing arc) | `A-DOOR` |
| `wall.window` | Window opening (jamb gap + mullion lines) | `A-GLAZ` |
| `grid.axis` | Axis grid with bay/row widths + bubbles | `S-GRID` |
| `column.place` | Columns at grid intersections | `S-COLS` |
| `stair.plan` | Straight or dog-leg stair with concrete quantity | `S-STRS` |
| `param.wall.new` / `param.commit` | Parametric wall (Rust graph, measured facts, take-off) | `A-WALL` |
| `sheet.designer` | Title plate layout with title block | — |
| `sheet.fields` | Title block attribute values | — |
| `level.new` / `level.activate` | Named model spaces | — |
| `sheetset.new` / `sheetset.activate` | Discipline annotation spaces | — |
| `project.setup` | Project with floors, heights, layer sets, tabs | — |

---

## Phases

### Phase 1 — New node types (O-Dynamo only) — **done**

**Goal:** The companion graph can produce doors, windows, columns, stairs and grid axes, not
just wall outlines and rectangles.

**Verified:** `dotnet test` on `ODraft.Graph.Tests` — 9 passed (3 existing + 5 node-type + 1 combined).
Sample graph: `ODynamo.Host/graphs/plan-elements.json`.

| File | Change |
|---|---|
| `ODraft.Geometry/Geom2D.cs` | `Arc2`, `DraftGeom`, `GridGeom`; `DoorOpening`, `WindowOpening`, `ColumnRect`, `StairStraight`, `GridAxes` |
| `ODraft.Graph/GraphEngine.cs` | Nodes `door`, `window`, `column`, `stairStraight`, `gridAxis`; `GraphArc`; `GraphResult.Arcs` |
| `ODynamo.Host/PipeServer.cs` | Evaluate response includes `arcs` |
| `ODraft.Graph.Tests/GraphTests.cs` | One test per new node type plus a combined graph |

**Pipe output:** `result.arcs: [{cx, cy, r, startAngle, endAngle, layer}]` (radians, CCW from +X).

**No Tren changes.** The engine does not need to know about arcs from the companion until
Phase 3.

---

### Phase 2 — Pipe protocol v2: context and subscription (both repos) — **done**

**Goal:** Tren sends drawing context (levels, project details) to the companion; the
companion can push live results without polling.

#### 2a — Context methods (Tren → Host)

Add two request methods to the pipe protocol:

| Method | Payload (from Tren) | Purpose |
|---|---|---|
| `context.levels` | `{ levels: [{name, elevation, height, active}] }` | Graph nodes can target a level |
| `context.project` | `{ name, owner, architect, units, scale, floors, … }` | Graph nodes can read project fields |

**O-Dynamo changes:**

| File | Change |
|---|---|
| `PipeServer.cs` | Handle `context.levels` and `context.project`: store them in `_levels` and `_project` fields. Pass them to `GraphEngine.Evaluate` as an optional `GraphContext` parameter. |
| `ODraft.Graph/GraphContext.cs` | New record: `GraphContext(IReadOnlyList<LevelInfo> Levels, JsonElement Project)`. |
| `ODraft.Graph/GraphEngine.cs` | `Evaluate` accepts `GraphContext?`. New node types: `levelSelect` (dropdown, value is a level name, outputs the name as a string and the elevation/height as numbers), `projectField` (reads a named field from the project JSON). |
| Tests | `levelSelect` outputs the level name; `projectField` reads the project title. |

**Tren changes:**

| File | Change |
|---|---|
| `crates/engine/src/cmd/companion.rs` (new) | `companion.connect {pipe?}` — opens the named pipe, sends `context.levels` and `context.project`, stores the pipe handle in `Session.companion`. `companion.disconnect`. `companion.evaluate` — calls `evaluate`, returns lines/circles/arcs. `companion.status` — reports connected/disconnected. |
| `crates/engine/src/cmd/mod.rs` | Register `companion::specs()`. |
| `apps/aad-winui/MainWindow.xaml.cs` | Menu item: Architecture > Companion > Connect / Disconnect / Evaluate / Commit. |
| Tests | `companion.connect` with no host returns a clear error, not a crash. |

The `ParametricBridge.cs` that was removed is **not restored**. The new `companion.*` commands
are engine commands, not UI code. The WinUI shell invokes them through `app.run`.

#### 2b — Live subscription (Host → Tren)

| Method | Direction | Payload |
|---|---|---|
| `subscribe` | Tren → Host | `{}` — the host will push `evaluation` messages whenever the graph changes (debounced 300 ms). |
| `evaluation` | Host → Tren (push) | Same shape as `evaluate` response. |
| `unsubscribe` | Tren → Host | Stop pushing. |
| `drawing.changed` | Tren → Host | `{ selection?, layers?, activeLevel? }` — optional, so `geom.path`-style nodes could track source entities. |

**O-Dynamo changes:**

| File | Change |
|---|---|
| `PipeServer.cs` | Refactor from one-shot (read line, write line, close) to a persistent duplex connection. On `subscribe`, set a flag; on `GraphBox.TextChanged` (debounced), push an `evaluation` to the writer. On `unsubscribe` or disconnect, clear the flag. |
| `MainWindow.xaml.cs` | Wire `GraphBox.TextChanged` → debounced push. Show "Tren connected" in the status bar. |

**Tren changes:**

| File | Change |
|---|---|
| `companion.rs` | `companion.connect` keeps the pipe open. A background task reads pushed `evaluation` messages and stores them in `Session.companion_preview`. The canvas draws the preview as a ghost overlay (faded, on a synthetic `_COMPANION` layer, not selectable). |
| `crates/aad-host/src/lib.rs` | Expose the companion preview lines to the renderer (same path as `param.evaluate` preview). |

**Verified:** Context nodes and pipe methods work; `companion.connect` without a host errors
clearly; subscribe stores pushed evaluations into `Session.companion` / `companion.preview`.
Ghost overlay: `companion.preview { result }` / evaluate stash feeds `Session::companion_ghost`
drawn faded on the host overlay (`_COMPANION`, not selectable); `aad-host` overlay unit test.

---

### Phase 3 — Level-aware commit (both repos) — **done**

**Goal:** Geometry from the companion lands on the correct level instead of always the first
model space.

**Verified:** O-Dynamo level-on-output test; `companion.commit` with Ground/First lines+circle,
store counts, and undo (`cargo test --test companion`: 8 passed).

Every output item has an optional `level` field (from a wired `levelSelect` or a string literal).
If absent, commit uses the active level (or `Level 1`).

| File | Change |
|---|---|
| `GraphLine` / `GraphCircle` / `GraphArc` | Optional `Level` |
| `GraphEngine.cs` | Geometry nodes read `level` port / literal |
| `companion.commit` | Groups by level, creates missing levels, writes lines/circles/arcs, one undo step |

---

### Phase 4 — Smart-block insertion and title-plate fields (both repos) — **done**

**Verified:** O-Dynamo `blockInsert` / `titleField` tests; `companion.commit` places a known
block on Ground (warns and skips missing names), writes `REV`/`SCALE` onto the title block
(`cargo test --test companion`: 10 passed; `dotnet test`: 14 passed).

**Goal:** The graph can insert smart blocks from the discipline library and populate title-block
fields.

#### 4a — Smart-block insert node

**O-Dynamo changes:**

| File | Change |
|---|---|
| `GraphEngine.cs` | New node type `blockInsert`: inputs `name` (block name from the library), `x`, `y`, `rotation`, and parameter ports (`width`, `height`, `type`, etc.). Output: a `GraphInsert` record. |
| `ODraft.Graph/GraphInsert.cs` | `record GraphInsert(string BlockName, double X, double Y, double Rotation, Dictionary<string, string> Attributes, string? Level)`. |
| `GraphResult` | Gains `IReadOnlyList<GraphInsert> Inserts`. |
| Tests | `blockInsert` node produces one insert with the given attributes. |

**Tren changes:**

| File | Change |
|---|---|
| `companion.rs` | `companion.commit` handles `inserts` by calling the existing `insert` command for each, setting attributes from the dictionary, on the correct level. If the block name is not in the drawing, report a warning (do not crash). |

#### 4b — Title-plate field output

**O-Dynamo changes:**

| File | Change |
|---|---|
| `GraphEngine.cs` | New node type `titleField`: inputs `field` (one of SCALE, REV, DATE, DRAWN_BY, etc.) and `value`. Output: a `GraphTitleField` record. |
| `GraphResult` | Gains `IReadOnlyList<GraphTitleField> TitleFields`. |

**Tren changes:**

| File | Change |
|---|---|
| `companion.rs` | `companion.commit` calls `sheet.fields` with the collected title-field values. |
| Tests | A graph that sets `REV` to "A" results in the title block showing "A". |

**Done when:** A graph places a door block on "Ground" and sets the sheet revision to "B";
both appear in the drawing after commit.

---

### Phase 5 — Sheet-set annotation output (both repos) — **done**

**Verified:** `textNote` / `dimLinear` on Electrical for Ground land in the annotation store;
design store stays empty; design mode restored after commit (`cargo test --test companion`: 11
passed; `dotnet test`: 15 passed).

**Goal:** Text, dimensions and leaders from the graph go into a sheet-set annotation store, not
the design geometry.

**O-Dynamo changes:**

| File | Change |
|---|---|
| `GraphEngine.cs` | New node types: `textNote` (inputs: text, x, y, height, rotation, layer, level, sheetSet), `dimLinear` (inputs: p1, p2, offset, layer, level, sheetSet). Output: `GraphText`, `GraphDimension` records. |
| `GraphResult` | Gains `Texts` and `Dimensions` lists. |

**Tren changes:**

| File | Change |
|---|---|
| `companion.rs` | `companion.commit` switches to the named sheet set + level target (using `sheetset.activate` and `level.activate`), writes the text/dimension entities, then restores the original target. |
| Tests | A dimension on sheet set "Electrical" for level "Ground" lands in the annotation store, not the design store. |

**Done when:** `drawing.inspect` confirms the dimension is in the Electrical annotation for
Ground, and switching to Design mode does not show it.

---

### Phase 6 — WinUI graph editor (Tren only, optional) — **done** (visual + JSON)

**Shipped:** Side rail **Graph** tab with node canvas (drag cards, add wall/door/number,
double-click to edit JSON fields), JSON mode, Evaluate and Commit. When the sibling `O-Dynamo`
repo is present (`ODRAFT_GRAPH`), Evaluate uses in-process `GraphEngine` and stashes
`companion.preview` for the canvas ghost; Commit uses `companion.commit`. The separate WPF
host remains available.

**Goal:** Replace the WPF JSON editor with a node-graph panel inside the WinUI shell, removing
the separate process and the pipe.

**Approach:**

1. Reference `ODraft.Graph` and `ODraft.Geometry` directly from `Aad.WinUi.csproj` (they are
   netstandard2.0/net8.0, compatible with the WinUI project).
2. Build a `GraphPanel.xaml` that renders nodes as `Border` elements on a `Canvas`, ports as
   small circles, edges as `Path` elements. Use the WinUI design system tokens
   (`Density.CornerControl`, `Density.GapStandard`, `Tokens.SurfaceSecondary`).
3. `GraphEngine.Evaluate` runs in-process; no pipe. Results go directly to
   `companion.commit` via `app.run("companion.commit", json)`.
4. The side panel (right rail) gains a "Graph" tab. When collapsed, the rail shows "Graph";
   when open, the full node editor is visible.

**O-Dynamo remains buildable** as a standalone tool for users who prefer the separate window
or use it without Tren.

**Done when:** A wall + door graph can be built entirely in the Graph window, committed to the
drawing, and undone. The WPF host still works independently.

**Verified:** WinUI builds with `ODraft.Graph.dll` when O-Dynamo is a sibling; **Window > Graph**
ships Nodes + JSON modes as a separate manager window (not on the side rail).

---

## Phase dependency graph

```
Phase 1 (nodes)
    ↓
Phase 2a (context) ──→ Phase 2b (subscription)
    ↓                       ↓
Phase 3 (level commit) ←────┘
    ↓
Phase 4a (block insert)   Phase 4b (title fields)
    ↓                         ↓
Phase 5 (annotation) ←───────┘
    ↓
Phase 6 (WinUI editor, optional)
```

Phases 1 and 2a are independent and can run in parallel.

---

## Work estimates

| Phase | O-Dynamo | Tren | Tests | Total |
|---|---|---|---|---|
| 1 — New nodes | Geometry + Engine: 5 node types, ~200 LOC | — | 5 tests | Small |
| 2a — Context | PipeServer + GraphContext: ~120 LOC | companion.rs: ~250 LOC, menu items | 4 tests | Medium |
| 2b — Subscription | Duplex pipe refactor: ~180 LOC | Background reader + ghost overlay: ~200 LOC | 2 tests | Medium |
| 3 — Level commit | Level field on output records: ~30 LOC | Level-grouped commit: ~150 LOC | 3 tests | Small |
| 4a — Block insert | GraphInsert + blockInsert node: ~80 LOC | Insert handling in commit: ~80 LOC | 2 tests | Small |
| 4b — Title fields | GraphTitleField + node: ~40 LOC | sheet.fields call: ~30 LOC | 1 test | Small |
| 5 — Annotation | Text/dimension nodes: ~120 LOC | Sheet-set target switch: ~100 LOC | 2 tests | Medium |
| 6 — WinUI editor | — | GraphPanel.xaml + canvas: ~800 LOC | 3 UI tests | Large |

---

## Risks

| Risk | Mitigation |
|---|---|
| Pipe protocol breaks between versions | Version field (`v: 2`) on every message; host ignores unknown methods; Tren falls back to v1 if `context.*` is refused |
| Level names in the graph drift from the drawing | `context.levels` is sent on every connect and on every `level.new` / `level.set` / `level.delete`; the graph validates level names against the last context |
| Block names from the graph don't match the drawing | `companion.commit` warns (does not crash) and skips unknown blocks; the user sees which blocks are missing |
| WPF and WinUI in one solution | Phase 6 does not replace O-Dynamo; it adds an alternative. The pipe stays for the standalone host |
| Ghost preview performance | Preview lines are on a synthetic non-selectable layer; the renderer treats them like traces (cached, faded, no grips). Budget: 10 000 preview lines at 60 fps |

---

## Testing strategy

Every phase lands with:

1. **Unit tests** in `ODraft.Graph.Tests` (`dotnet test`) for every new node type and context
   method.
2. **Engine tests** in `crates/engine/tests/companion.rs` for `companion.connect` (mock pipe
   or error), `companion.commit` (entities on the right level/sheet-set), and hostile inputs
   (NaN coordinates, missing levels, empty graph).
3. **Integration test** via the control protocol: start Tren with `--control`, start
   `ODynamo.Host`, send `companion.connect` + `companion.evaluate` + `companion.commit` +
   `drawing.inspect`, assert entity counts.
4. **Quality gate:** `cargo xtask ci` (Rust) and `dotnet test` (C#) both pass before any
   commit.

---

## Documentation impact

| Doc | Update |
|---|---|
| [parametric.md](parametric.md) | Add the companion section back with the new protocol and node types |
| [status.md](status.md) | Companion graph row: list phases as implemented vs planned |
| [architecture/DYNAMO_INTEGRATION.md](architecture/DYNAMO_INTEGRATION.md) | Rewrite with the v2 protocol, context, subscription, and level-aware commit |
| [architecture/GEOMETRY_BACKEND.md](architecture/GEOMETRY_BACKEND.md) | Note the expanded `Draft2D` library |
| [control-protocol.md](control-protocol.md) | Add `companion.*` methods |
| [licensing/DEPENDENCY_INVENTORY.md](licensing/DEPENDENCY_INVENTORY.md) | No new dependencies (all code is ours) |
| This file | Mark each phase done as it lands |
