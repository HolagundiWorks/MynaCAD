# Geometry database: turn the drawing into numbers

Status: **architecture draft, 2026-11-08.** Nothing in this file is built. It answers: how do we make Tren understand a plan by arithmetic on stored coordinates (two parallel lines 230 mm apart are a wall) instead of by looking at it, and how do the AI and Falcon use the result.

## 1. Idea in one paragraph

A 2D plan is lines, arcs, circles, blocks, hatches and text, each with coordinates. Convert the drawing, once and incrementally, into **tables of vectors in millimetres** (start point, end point, length, angle, layer, bounding box). Then every question becomes a query over numbers: "pairs of parallel segments whose gap is 230 mm ± 5 and that overlap by at least half their length" is a wall; "a closed 230 x 230 rectangle" is a column; "a block named `WC*` within 20 mm of a pipe end" is a water closet on that pipe. The rules are parameters in a table, not code and not a model. The language model never sees the picture; it reads counts and the short list of items the rules could not decide, and proposes parameter changes or labels.

## 2. Principles

1. **Numbers first, pictures never.** The classifier input is coordinates. Layers, block names, linetypes and text are extra evidence with weights, because real drawings put everything on layer `0`.
2. **Derived, rebuildable, never the source of truth.** The drawing bytes in the `.osp` stay exactly as they are. The geometry tables can be dropped and rebuilt at any time and are rebuilt from the drawing, so there is no migration problem and no second truth.
3. **Deterministic and explainable.** Same drawing and same rule set give the same result. Every classified item records the rule, the rule version, a confidence and the primitives it was built from.
4. **Nothing changes the drawing silently.** Classification writes only to the geometry tables. Moving objects to layers is a separate step: a preview table (handle, from layer, to layer, rule, confidence), then one undoable command.
5. **Parameters live in a table.** Wall thicknesses, tolerances and name patterns are rows a person (or the AI, through a proposal) can edit; each change bumps `rules_version` and reclassifies.
6. **Fast on the heavy file.** The 232,212 entity drawing is the test: the build is bulk, off the UI thread and incremental after edits.

## 3. Where it lives

Recommended: **tables in the project `.osp`** (SQLite, prefix `geo_`), so Tren, Falcon and Kivo's importer read one file with no second database to keep in step. The `change` log and the cloud push skip `geo_*` (they are derived and large); Myna's service syncs the drawing and the take-off, not the geometry. Open decision in section 11: a sidecar `<name>.osp-geo` instead.

## 4. Pipeline

```
 drawing (Arc<Entity> store, chunked, persistent)
        |
  [1 extract]   walk model space, expand blocks with their transforms, units to mm,
        |       polylines to segments, bulge to arcs, text and inserts kept
        v
  geo_prim / geo_seg / geo_arc / geo_insert / geo_text      (+ R-tree on bounding boxes)
        |
  [2 derive]    parallel pairs, closed loops, endpoint graph, collinear runs   (pure geometry)
        v
  geo_pair / geo_loop / geo_node / geo_edge
        |
  [3 classify]  rules (table) turn derived shapes into features with confidence
        v
  geo_feature / geo_feature_member      walls, openings, columns, beams, slabs, rooms,
        |                                hatches, electrical points, plumbing fixtures and pipes
  [4 network]   endpoint snapping (20 mm), fixture-to-pipe-to-source graphs, circuits
        v
  geo_conn
        |
  [5 report / apply]   report tables, preview of layer moves, take-off rows, AI tools
```

Incremental update: `EntityStore::changed_since` and `removed_since` already give the changed and erased handles from chunk pointers. Stage 1 re-extracts only those handles; stages 2 to 4 re-run only inside the affected bounding boxes (found through the R-tree), so an edit costs the edit, not the drawing.

## 5. Schema (draft)

All lengths in **millimetres, REAL**. Every coordinate also has a quantised twin (`round(x * 10)`, 0.1 mm integer) used for exact joins and hashes, written `qx1` and so on; they are omitted below.

| Table | Columns | Meaning |
|---|---|---|
| `geo_run` | id, built_utc, drawing, drawing_sha256, revision, unit_scale, rules_version | one row per build; tells whether the tables match the drawing |
| `geo_prim` | id, handle, kind (`line`,`arc`,`circle`,`poly`,`hatch`,`point`), layer, color, linetype, lineweight, closed, space, block_path, minx, miny, maxx, maxy | one row per source entity (or per entity inside an expanded block) |
| `geo_seg` | id, prim_id, seq, **x1, y1, x2, y2**, len, angle (0 to 180), nx, ny | **every straight piece as a vector**: a line is one row, a polyline is one row per edge; `angle` is the direction folded to 0..180, `nx, ny` the unit normal |
| `geo_arc` | id, prim_id, cx, cy, r, a0, a1 | arcs and circles (door swings, round columns, bends) |
| `geo_insert` | id, prim_id, block, x, y, rot, sx, sy, width, depth, shape_hash, attrs_json | blocks as placed: footprint size and a hash of the block's own geometry for matching |
| `geo_text` | id, prim_id, text, x, y, height, rot | tags like `D1`, `W2`, `WC`, `S1`, room names |
| `geo_hatch` | id, prim_id, pattern, solid, area, loops_json | hatch boundary loops as polygons |
| `geo_rtree` | virtual `rtree(id, minx, maxx, miny, maxy)` | spatial index over `geo_prim` |
| `geo_pair` | id, seg_a, seg_b, **gap**, overlap, overlap_ratio, axis_x1, axis_y1, axis_x2, axis_y2 | two parallel segments close enough to be the two faces of something, with their shared centre line |
| `geo_loop` | id, kind, area, perimeter, w, h, points_json | closed rings (rectangles, rooms) found from the segment graph |
| `geo_node` / `geo_edge` | node: id, x, y; edge: id, seg_id, a, b, length | endpoint graph after snapping (tolerance in the rule table) |
| `geo_rule` | id, kind, enabled, params_json, version, weight | the parameter table (section 6) |
| `geo_feature` | id, kind, subkind, confidence, rule_id, rules_version, x1, y1, x2, y2, width, depth, length, area, level, props_json | **the answer**: one row per wall, door, window, column, beam, slab, room, floor hatch, switch, fixture, pipe ... |
| `geo_feature_member` | feature_id, prim_id, role | which primitives built the feature (`face_a`, `face_b`, `swing`, `frame`, `tag`) |
| `geo_conn` | feature_id, to_feature_id, service, kind | fixture on pipe, pipe to source, switch to light |
| `geo_review` | feature_key, state (`accepted`,`rejected`,`edited`), note, by, at | a person's decision, keyed by the geometry hash so it survives a rebuild |

Segments are stored as **vectors with a start and an end point**, exactly as asked, so a line is two points and nothing is lost; direction, length and normal are precomputed because the rules compare them constantly.

## 6. Rules (all parametric, defaults in millimetres)

**Wall (double-line).** A `geo_pair` is built from segments that are
- parallel: `|angle_a - angle_b| <= 1.0` degree,
- a nominal gap apart: `|gap - t| <= 5 mm` for one thickness `t` in the thickness list, default **230** (9 in), **115** (4.5 in), 150, 200, 300 (12 in); the list is a rule parameter,
- overlapping along their length: `overlap >= max(2 * t, 600 mm)` and `overlap_ratio >= 0.6` of the shorter one.

Pairs on one axis that touch end to end merge into one **wall run** (a centre line, a thickness, a length). Corners and T-junctions join where face endpoints meet within `t`. Evidence that raises confidence: a layer named like `*WALL*`, a continuous linetype, a thickness that matches the drawing's majority thickness. Evidence that lowers it: a dashed linetype (then it is a **beam**, below), a pair shorter than 2 t, a pair crossing a larger wall.

**Wall (single line with a polyline width, or a solid hatch).** Treated as a wall when a polyline has a constant width equal to a thickness in the list, or a hatch of a solid pattern is a thin rectangle with a side equal to a thickness.

**Opening in a wall.** A wall run broken by a gap of 600 to 2400 mm. Door: a quarter-circle `geo_arc` with radius 600 to 1200 mm whose centre is on a wall face at the gap's end, or a block named `AADOPEN-*` / matching the smart-block door shapes. Window: the gap holds 2 to 4 thin parallel lines across the wall thickness (sill and glass lines), or an `AADOPEN-*` window block. Width, wall and offset from the nearest corner are stored.

**Column.** A closed loop that is a rectangle or circle with both sides between 150 and 900 mm (aspect <= 2.5), often hatched solid; or a block whose footprint fits that.

**Beam.** A pair like a wall (gap 150 to 450) drawn **dashed**, or on a layer matching `*BEAM*`; also a long thin rectangle with a text tag `B1`.

**Slab / floor.** A room loop (a closed face of the wall graph, section 7) with a floor hatch inside it; slab edge from the loop. **Floor hatch** versus **wall hatch**: a hatch whose boundary lies inside a room loop is a floor hatch; a hatch that covers a wall run's rectangle is a wall poche. The pattern name is evidence only.

**Rooms.** Faces of the planar graph formed by wall faces and opening gaps closed across their width; area and perimeter stored; the nearest `geo_text` inside is the room name.

**Electrical.** An insert is an electrical point when its block id or name matches the electrical block map (`electrical.blocks`), or its footprint and shape hash match a known symbol (switch, socket, light, fan, DB), with the nearest `geo_text` tag (`S1`, `L2`) as the label. Stored with the wall it sits on and its offset from the wall corner. Wiring: lines on the electrical layers between points, snapped at 20 mm, give circuits.

**Plumbing.** Fixtures from `plumbing.blocks` (water closet, basin, sink, shower, trap, tank, chamber, riser); pipes are segments on the service layers (cold, hot, drain) with the width as the diameter; nodes snapped at 20 mm; `geo_conn` links fixture to pipe to source per service, the same result `plumbing.connect` computes today but from the stored numbers.

**Scale check (runs first).** The median of the pair gaps is compared with the thickness list. A median near 0.23 means metres, near 9 means inches, near 0.0075 means a scaled sheet: the build warns and offers the unit scale instead of classifying nonsense.

## 7. Finding pairs without comparing everything

Comparing every segment with every other is quadratic and would not finish on 232,000 entities. Instead:

1. Bucket segments by folded angle (0.5 degree buckets).
2. Within a bucket, project each segment onto its normal: `d = x * nx + y * ny`; sort by `d`.
3. Slide a window of width `max gap + tolerance` along the sorted list; only segments inside the window are candidates.
4. For each candidate pair, check overlap along the axis with a sorted interval test, and look up the R-tree only to exclude pairs separated by other geometry.

That is `O(n log n)` plus the candidates actually found. The result is one `geo_pair` per true pair; the merge into runs and the corner joins use the node graph.

## 8. Using the result

- **Move to layers.** `geodb.apply` builds a preview from `geo_feature_member`: handle, current layer, target layer (`A-WALL`, `A-DOOR`, `A-WIND`, `S-COLS`, `S-BEAM`, `A-FLOR-HATCH`, `E-LITE`, `P-FIXT` ...), rule, confidence. A person accepts all above a confidence threshold or picks rows; the engine runs the existing layer commands as one undo step and stamps `BuildingElement` through `classify.set`.
- **Take-off.** Wall length, thickness, area, opening counts and sizes go to the existing `takeoff_row` kinds, so Falcon reads numbers it already understands.
- **AI.** The assistant gets read-only tools over the tables: `geo_summary` (counts by feature kind, mean confidence, the median thickness), `geo_ambiguous` (features under the threshold with their numbers), and `geo_propose` (a change to a rule parameter or a label for one feature, which goes through the same preview and confirmation). Because the input is a short table of numbers, a 7B model is enough for the part left to it; it never has to find a wall in a picture.
- **Checks.** Fixtures not connected, doors wider than their wall run, columns off a grid, walls thinner than the minimum: all plain queries.

## 9. Code layout

New crate `aad-geodb` (depends on `aad-doc`, `aad-geom`, `rusqlite` as `aad-odp` already does):

```
crates/geodb/src/
  schema.rs     create/drop the geo_* tables, versioned
  extract.rs    entity store -> prims, segs, arcs, inserts, text, hatches (blocks expanded, units to mm)
  derive.rs     pairs (angle buckets + sorted offsets), loops, node graph
  rules.rs      the parameter table, defaults, validation
  classify.rs   walls, openings, columns, beams, rooms, hatches, electrical, plumbing
  network.rs    snapping, connectivity
  update.rs     incremental rebuild from changed/removed handles
  apply.rs      preview and the layer/classify commands
  report.rs     summaries for the status bar, the AI tools and the take-off
```

Engine commands: `geodb.build`, `geodb.report`, `geodb.rules`, `geodb.apply`. Agent tools in `aad-agent`: `geo_summary`, `geo_ambiguous`, `geo_propose`.

## 10. How we will know it works (before any model is judged)

1. **Ground truth from real drawings.** Take drawings whose layers are already right (the demo villa, `Working Drawing.dwg`, the 232k-entity file), keep a copy of each object's true layer, set every layer to `0`, run the classifier and score: precision and recall per class (wall, door, window, column, beam, floor hatch, room, electrical point, plumbing fixture).
2. **Targets to agree before building:** for example wall recall and precision both at least 95 % on double-line plans, doors and windows at least 90 %, with every miss listed so the rule can be improved.
3. **Golden files.** A few small drawings with an expected `geo_feature` dump, checked in, so a rule change shows exactly what moved.
4. **Speed.** Build time and memory on the 232k file, and the cost of one edit through the incremental path.
5. **Only then the model.** The AI is measured on the leftover items (what the rules marked ambiguous): how often its label agrees with the ground truth, and whether it ever proposes something unsafe.

## 11. Open decisions

1. **Thickness list.** Is `230, 115, 150, 200, 300 mm` right, and which wins when a gap fits two? Should 9 in and 230 mm be one entry (they differ by 0.2 mm)?
2. **Storage.** In the `.osp` (one file, simple) or a sidecar next to it (keeps the project file small and the sync light)? Recommendation: in the `.osp`, excluded from sync.
3. **Apply policy.** Accept above a confidence automatically after the first review, or always ask?
4. **Single-line walls.** Do your drawings draw walls as two lines mostly, or sometimes as one centre line with a width? The rules above handle both, but the tests should use your real files.
5. **Layer names.** Which layer standard should `apply` target: the existing `A-WALL`, `A-DOOR` style, or a new list?

## 12. Phases

| Phase | Delivers | Done when |
|---|---|---|
| 0 | schema, extract, R-tree, scale check | the 232k file builds in a bulk transaction and the tables match entity counts |
| 1 | pairs, wall runs, openings | wall and opening scores on the ground-truth set reach the agreed target |
| 2 | columns, beams, rooms, hatches, slab | same, per class |
| 3 | electrical and plumbing points and networks | `plumbing.connect` and the stored graph agree |
| 4 | apply with preview, review states, status-bar summary | a drawing with every layer `0` becomes correctly layered in one undoable step |
| 5 | AI tools over the tables, model comparison | measured on the ambiguous items; decide the model from numbers |
