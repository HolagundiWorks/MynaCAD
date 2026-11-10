# Geometry database: turn the drawing into numbers

Status: **phases 0, 1 and 2 are built** (crate `aad-geodb`, commands `geodb.*`); phases 3 to 5 are planned. Measured results and what is not handled yet: [GEOMETRY-DB-RESULTS.md](GEOMETRY-DB-RESULTS.md). The owner's decisions of 2026-11-08 are in section 2 and are part of the design.

## 1. Idea in one paragraph

A 2D plan is lines, arcs, circles, blocks, hatches and text, each with coordinates. The drawing is converted, once and then incrementally, into **tables of vectors in millimetres** (start point, end point, length, angle, layer, bounding box). Every question then becomes arithmetic over numbers: "two parallel segments 230 mm apart that share at least 100 mm of length and chain into a run of 600 mm" is a wall; "a quarter-circle of radius 500 to 1300 mm whose centre sits on a wall" is a door swing. The rules are parameters in a table, not code and not a model. The language model never sees the picture (phase 5).

## 2. Decisions (owner, 2026-11-08)

1. **Units, not a fixed thickness list.** The drawing carries the unit it was drafted in. Metric drawings use 230 mm for a full-brick wall, imperial drawings 9 in (228.6 mm). Wall types are three: **half brick** (115 mm / 4.5 in), **full brick** (230 mm / 9 in) and **cavity wall** (default overall 300 mm / 12 in = 115 leaf + 70 cavity + 115 leaf; **the 300 mm default is an assumption about local practice, not a code value**, and is an editable rule parameter). Walls are always drawn as two lines: the single-line-with-width and solid-hatch wall rules are dropped. All geometry tables are stored in millimetres whatever the drawing unit.
2. **Storage.** All files stay local. The tables live in the project `.osp` (Tren and Falcon share this local database). Nothing from `geo_*` is synced anywhere: the tables are not written to the `change` log and no push reads them. Only Kivo syncs to the cloud, and company data reaches Kivo only through the CSV Falcon exports.
3. **Apply policy: review first, then approve.** Classification never moves anything. `geodb.apply` makes a preview (handle, from layer, to layer, rule, confidence); only an explicit `approve: true` runs the layer changes, as one undo step. No auto-apply.
4. **Layers.** The standard layer set is fixed in the app and not user-configurable (section 8).
5. **Foreign drawings.** A drawing from another CAD program is detected by origin and must go through the layer mapper before anything is applied (section 9).

## 3. Principles

1. **Numbers first, pictures never.** The classifier input is coordinates. Layers, block names and linetypes are extra evidence with weights, because real drawings put everything on layer `0`.
2. **Derived, rebuildable, never the source of truth.** The drawing in the `.osp` is untouched. The derived tables can be dropped and rebuilt at any time (`geo_schema` version mismatch drops them). Only `geo_rule` and `geo_review` hold a person's data and survive.
3. **Deterministic and explainable.** Same drawing and same rules give the same result. Each feature records its rule, rule version, confidence and the primitives it came from.
4. **Nothing changes the drawing silently.** Build and report write only to the tables.
5. **Parameters live in a table.** Editing a rule bumps `rules_version` and reclassifies.
6. **Fast on the heavy file.** Bulk build, off the UI thread (`aad_engine::cmd::geodb::spawn_build`), incremental after edits.

## 4. Units and the scale check

`aad_geodb::units`. The drafting unit comes from, in order: an explicit `units` argument of `geodb.build` (confirms), the drawing's own record (project key `draftingUnits`, written by Tren), `INSUNITS` (1 in, 2 ft, 4 mm, 5 cm, 6 m), and last the **scale check**. A drawing with no units is **never guessed**: the scale check proposes a unit, the run is marked `units_confirmed = 0`, a warning is returned and `geodb.apply` refuses until `geodb.build {units}` confirms it.

Scale check: for each candidate unit (mm, in, m, cm, ft) the sampled line length (the 40,000 longest segments) is paired at that unit's wall thicknesses; the score is the share of line length that pairs up. The best score names the unit when it is clearly ahead (1.5 times the runner-up, at least 1 %); the median pair gap in millimetres is reported. A declared unit whose score is far below the best is flagged ("the drawing says in but its wall thicknesses fit mm much better").

Where the unit is recorded: new drawings in Tren get `draftingUnits` (`{"system":"metric","unit":"mm"}` or inches) and the Tren stamp at creation; every save writes the stamp and, when the drawing declares its units, `draftingUnits`; saving into an `.osp` also writes `drafting_units`, `drafting_unit` and `origin` into the project `meta` table (`write_project_meta`). Imported drawings read `INSUNITS` / `MEASUREMENT`.

## 5. Pipeline

```
 drawing (Arc<Entity> store, chunked, persistent)
        |
  [1 extract]   model space; blocks expanded with their transforms; units to mm;
        |       polyline edges to one geo_seg each, bulges to geo_arc, inserts, text, hatches
        v
  geo_prim / geo_seg / geo_arc / geo_insert / geo_text / geo_hatch   (+ geo_rtree, quantised twins)
        |
  [2 derive]    parallel pairs: angle buckets + sorted normal offsets (no quadratic compare)
        v
  geo_pair
        |
  [3 classify]  columns, wall runs, joins, door swings and blocks, windows    (rules from geo_rule)
        v
  base features (what the geometry of each piece says)
        |
  [4 context]   page regions, rooms and slab on a raster, plan / structure / other,
        |       beams, wall confidence from rooms, hatch assignment                 (whole sheet)
        v
  geo_feature / geo_feature_member / geo_region / geo_room / geo_hatch_link
        |
  [5 report / apply]   geodb.report, geodb.apply preview and approve
```

(Networks, electrical and plumbing, are phase 3 and would be a later stage.) Stage 3 works inside a window around an edit; stage 4 looks at the whole sheet and is recomputed from the base features after every update, which is why an incremental result equals a full rebuild (the cost is in the results document). Incremental update: `EntityStore::changed_since` / `removed_since` give the changed and erased handles; only those entities are re-extracted. Features touching the edit (inflated by what one face can pair across) are dropped and re-derived whole inside a window around them; nothing else is touched. Changed block definitions mark their inserts dirty. A change of `INSUNITS` / `MEASUREMENT` triggers a full rebuild. The stored copy is updated with `store::write_update` (delete the dirty handles' rows, insert the new ones).

## 6. Schema (version 2)

All lengths millimetres, REAL. Coordinates also have a quantised twin (`round(v * 10)`, 0.1 mm integer): `qx1 qy1 qx2 qy2` on `geo_seg`, `qcx qcy qr` on `geo_arc`. A block's expanded geometry carries the **top-level insert's handle** (the thing that can be moved) and the block path.

| Table | Columns | Notes |
|---|---|---|
| `geo_schema` | key, value | schema version |
| `geo_run` | id, built_utc, drawing, drawing_sha256, revision, unit_scale, unit, system, units_confirmed, rules_version, info_json | one row; the sha256 is a fingerprint of the geometry rows |
| `geo_prim` | id, handle, kind, layer, color, linetype, lineweight, closed, space, block_path, minx miny maxx maxy | one row per entity or per entity inside an expanded block |
| `geo_seg` | id, prim_id, seq, **x1 y1 x2 y2**, len, angle (0..180), nx, ny, q* | every straight piece as a vector |
| `geo_arc` | id, prim_id, cx, cy, r, a0, a1, q* | arcs and circles (degrees, counter-clockwise) |
| `geo_insert` | id, prim_id, block, x, y, rot, sx, sy, width, depth, shape_hash, attrs_json, nested | blocks as placed, with attributes (`OKIND`, `OWIDTH`) |
| `geo_text` | id, prim_id, text, x, y, height, rot | |
| `geo_hatch` | id, prim_id, pattern, solid, area, loops_json | hatch boundaries are not segments (they would double the wall faces) |
| `geo_rtree` | virtual `rtree(id, minx, maxx, miny, maxy)` | over `geo_prim` |
| `geo_pair` | id, seg_a, seg_b, gap, overlap, overlap_ratio, axis_x1.., band, ladder | two parallel faces and their centre line |
| `geo_rule` | id, kind, enabled, params_json, version, weight | one row per rule section; **survives a rebuild** |
| `geo_feature` | id, feature_key, kind, subkind, confidence, rule_id, rules_version, x1 y1 x2 y2, width, depth, length, props_json | wall, beam, column, door, window, room, slab. A room or slab stores its extent in `x1 y1 x2 y2`, its perimeter in `length`, and `area_mm2` in `props_json` |
| `geo_feature_member` | feature_id, prim_id, handle, role | `face`, `cap`, `section` (column), `swing`, `leaf`, `block`, `sill` |
| `geo_region` | id, minx miny maxx maxy, kind, score, walls, wall_len, doors, rooms, closure, items | page regions: `plan`, `structure`, `other`, `unjudged`, `empty` (no wall runs) |
| `geo_room` | feature_id, feature_key, kind (`room` or `slab`), region_id, area_mm2, perimeter_mm, loop_json, hatches | closed loops with their outline polygon in mm |
| `geo_hatch_link` | hatch_id, prim_id, handle, kind (`room`, `wall`), feature_key | which room or wall a hatch sits in |
| `geo_review` | feature_key, state, note, by, at | accepted / rejected / edited, keyed by the geometry hash; **survives a rebuild** |

`geo_region`, `geo_room` and `geo_hatch_link` are derived from the whole sheet and are replaced as a set on every write. Not yet built: `geo_node`, `geo_edge`, `geo_conn` (phase 3). The schema version went from 1 to 2: a file with version 1 has its derived tables dropped and rebuilt; `geo_rule` and `geo_review` stay.

## 7. Rules (`geo_rule`, defaults in millimetres)

Sections: `wall_types`, `cavity_parts`, `pair`, `door`, `window`, `evidence`, `column`, `beam`, `room`, `region`. `geodb.rules` shows and edits them; every edit is validated (finite, bands of the wall types must not overlap) and bumps the version.

**Wall pair.** Two segments are a pair when they are parallel (`angle_tol_deg` 1.0), their gap is within `tolerance_mm` (8) of a wall type's thickness for the drawing's unit system, they share at least `min_overlap_mm` (100) along the axis and at least `min_overlap_ratio` (0.6) of the shorter one, and they are not two parts of one block. The gap must hold at both ends of the shared part. Real plans draw walls in many short pieces, so the pair threshold is low and the length test is on the **run**.

**Wall run.** Pairs with the same type, direction (half a degree) and centre line (within 6 mm) whose intervals touch within `join_gap_mm` (25) merge into one run: centre line, measured and nominal thickness, type, length. Runs shorter than `min_run_mm` (600) are dropped. Joins: the end of a run within `node_tol_mm` of another run's rectangle is a **corner** or a **T** (recorded in `props.joins` by feature key). Lines across the wall (end caps and jambs: as long as the wall is thick, square to it, both ends on the faces) become members with role `cap`.

**Confidence.** `base_wall` 0.55; plus `layer_match` for a layer named like `*WALL*`, `long_run_factor` for a run of at least 3 thicknesses (otherwise `short_penalty`), `joined` for corners; minus `ladder_penalty` (a face with partners on both sides at the same gap: stair treads, hatch), `dashed_penalty` (dashed or hidden faces: a beam or centre line), `nested_penalty` (a run wholly inside a thicker one: window sill lines), `column_penalty` (both faces are sides of one small closed rectangle), `block_penalty` (default 0). Features under `classify_min` (0.5) are kept and listed as ambiguous but are not classified.

**Door.** A quarter-circle (sweep within 12 degrees of 90) of radius 500 to 1300 mm whose centre (hinge) lies within 25 mm of a wall run's rectangle (confidence 0.70, +0.20 when a swing end also lies on a wall, +0.10 for a leaf line of the radius length from the hinge; without a wall at the hinge it stays below the threshold). Or a block whose attribute `OKIND` is `door`, or whose name matches `door.block_patterns` (`*DOOR*`, `DR-*`). Width, wall and offset from the run end are recorded.

**Window.** A block with `OKIND` `window` or a name matching `window.block_patterns`; or, along a wall run, 2 to 5 thin lines parallel to it, 450 to 3000 mm long, that start and stop at the same places, span at least half the thickness and either include a line between the two faces (a sill) or fill the gap between two wall ends. A door leaf in the opening is not a window. The contents of door and window blocks are the symbol and take no part in wall or swing detection.

**Column** (`column` section). A closed polyline or four separate lines that meet at square corners (within `corner_tol_mm`, sides `min_side_mm` to `max_side_mm`, longest over shortest at most `max_aspect`), or a circle of radius `round_r_min_mm` to `round_r_max_mm`. Confidence: `base` 0.25, `layer_match` (+0.3, layer like `*COL*`), `typical_bonus` (+0.15, sides 200 to 600 mm and nearly square), `fill_bonus` (+0.2, a hatch or solid centre lies inside), `near_wall_bonus` (+0.05, a wall run passes within reach), `structure_bonus` (+0.15, in a structural layout region); circles take `round_penalty`. Rectangles with confidence at or above `evidence.classify_min` leave the wall candidates (their lines are not paired); a doubtful rectangle stays a candidate, because a short wall piece is also a small closed rectangle.

**Page regions** (`region` section). Every line, arc, text and hatch extent is marked on a grid of `gap_mm` (1500) cells and 8-connected cells form a region, so views drawn closer than about `gap_mm` to each other are one region; lines longer than `frame_share` of the sheet diagonal and at least `frame_min_mm` (a sheet frame) are left out. Each region with wall runs is judged on its wall runs:

- **plan**: it has at least one room, at least `min_wall_mm` of wall, at least `plan_min_doors` door features, and a plan score of at least `plan_min_score`, where the score is (wall length touching a room / wall length) + `door_weight` per door swing (at most 2) + `room_weight` per room (at most 3). Window lines do not count, because beam layouts produce them.
- **structure**: no doors and at least `structure_min_columns` columns (a beam and column layout). Its pairs get `beam.structure` towards the beam score.
- **other**: the sheet has a plan and this region is neither (elevations, details, sections). Wall, column, door and window confidences lose `non_plan_penalty`.
- **unjudged**: no plan anywhere on the sheet (a small test drawing, a partial plan): nothing is penalised.

**Rooms and slab** (`room` section). The wall runs of a region (not dashed, confidence at or above `classify_min`) are drawn on a raster of `cell_mm` (50; larger if the region would exceed `max_cells`), each padded by half its thickness at the ends. Where a free wall end faces another wall within `bridge_mm` (1500) along its axis the gap is closed (a door or window opening). Empty cells not connected to the border are rooms (area between `min_area_mm2` and `max_area_mm2`); the area is cells times cell area, the perimeter the boundary length, the outline a simplified polygon. The filled shape (walls and rooms) is the **slab**, one per connected shape that contains a room and is at least `slab_min_area_mm2`.

**Wall evidence from rooms.** A wall run that separates a room from another room or the open sheet gets `room_bonus`; one that touches no room gets `isolated_penalty`; one with the same room on both sides and no joined wall gets `floating_penalty`; one with a hatch inside its outline gets `wall_hatch_bonus` (poche). These apply only in regions that have rooms.

**Beam** (`beam` section). A wall run becomes a `beam` feature when its score reaches `min_score` (0.5) and it is at least `min_length_mm` long. Score: `dashed` 0.6 (faces drawn dashed or hidden), `non_separating` 0.2 (the same room on both sides), `floating` 0.3 (and no joined wall), `over_wall` 0.3 (inside a thicker run), `across_wall` 0.3 (passes right through a wall running across it), `structure` 0.4 (in a structural layout), `layer_match` 0.3 (layer like `*BEAM*`). The evidence list is stored in `props.beam_evidence`. The region weight alone does not make a beam by default: a structural layout also shows the walls, and nothing in a pair tells the two apart without a linetype.

**Hatches.** `geo_hatch` keeps every hatch and solid boundary. A hatch whose centre sits in a room is linked to the room, one inside a wall run's outline to the wall (`geo_hatch_link`); the slab is the outline of the filled shape and its hatches are counted. Nothing is moved: hatches are not an `apply` target.

## 8. Layers (fixed standard set)

`geodb.apply` targets the app's existing standard layers (`crates/engine/src/cmd/hcw.rs`, `STANDARD_LAYERS`): walls `A-WALL`, doors `A-DOOR`, windows `A-WIND`. They are created with the standard look when missing. Phase 2 targets: columns `A-COL` (there is no `S-COLS` in the standard set; the earlier draft's name is not used), beams `S-BEAM`. `S-BEAM` was only created by the beam layout of the project wizard; `cmd/geodb.rs` keeps it in a small extra list (`EXTRA_LAYERS`) so it counts as standard and `geodb.apply` can create and target it. Nothing else was added. Rooms, slabs and hatch links are not moved to any layer. The list is not user-configurable.

## 9. Foreign drawings and the layer mapper

`geodb.origin` (and `origin` in the `open` result, and a command-line notice on open) reports whether a drawing is **foreign**: no `trenOrigin` stamp in the drawing, and either objects on layers outside the standard set (the union of the standard, presentation, permit and VH sets plus the room and layout layers) or every object on layer `0`. `$ACADVER` and `$DWGCODEPAGE` are reported as evidence but do not decide: Tren writes `AC1032` itself. Every drawing Tren saves carries the stamp (project key `trenOrigin`, also `origin` in the `.osp` `meta`); a drawing from an older Tren is foreign until it is saved once.

For a foreign drawing `geodb.build` still classifies (geometry does not need the layers) but returns `needsLayerMapping`, and `geodb.apply` refuses until the layer mapper has run. The mapper is the existing `layermap` command (`{map: {old: new}, create?, purge?}` moves objects, skips locked layers). This change adds `layermap {suggest: true}`, which lists candidate targets by keyword for the non-standard layers and changes nothing, and a project key `layerMapDone` that a mapping run sets. What the mapper still lacks is in the results document.

## 10. Using the result

- **Move to layers.** `geodb.apply` (no `approve`) builds the preview from `geo_feature_member` (walls to `A-WALL`, beams to `S-BEAM`, columns to `A-COL`, doors and windows to their layers). Rules: only features at or above `minConfidence` (default 0.6, never below `classify_min`), none rejected in `geo_review`; wall faces inside blocks are left alone (moving them would move the block); an object claimed by an opening and a wall goes to the opening; rows already on the target layer are dropped. `approve: true` applies the preview (optionally a subset by `handles`), refuses when the drawing changed since the preview, when units are unconfirmed or the drawing is foreign, skips objects on locked layers, and is one undo step. `BuildingElement` stamping through `classify.set` is not done yet.
- **Report.** `geodb.report` returns counts by kind, units, scale check, timings, warnings; `detail: ambiguous` lists the features just under the threshold with their numbers. `build` writes a one-line summary to the command log (the shell's status text).
- **Take-off and AI (later).** Wall length, thickness and opening counts to `takeoff_row`; read-only agent tools `geo_summary`, `geo_ambiguous`, `geo_propose` over the tables. Planned, not built.

## 11. Code layout

```
crates/geodb/src/
  units.rs     drafting units, INSUNITS, project key
  rules.rs     the parameter table, validation, wildcard matching
  model.rs     in-memory rows and features, quantised coordinates
  extract.rs   entity -> rows (blocks, transforms, caps and depth limits)
  derive.rs    parallel pairs
  classify.rs  wall runs, joins, caps, doors, windows (and calls columns.rs first)
  columns.rs   column rectangles (closed polylines, four lines) and circles
  context.rs   page regions, raster rooms and slab, plan / structure / other, beams, hatch links
  origin.rs    Tren stamp and foreign-drawing detection
  store.rs     the geo_* tables in the .osp (bulk write, incremental write, rules, reviews, meta)
  lib.rs       GeoModel: build, update, reclassify, scale check, summary
crates/engine/examples/geodb_groundtruth.rs  the ground-truth harness
crates/engine/src/cmd/geodb.rs         geodb.build / report / rules / review / origin / apply
```

`aad-geodb` sits at layer 2 (depends on `aad-geom`, `aad-doc`); persistence is compiled out on wasm.

## 12. How it is measured

`cargo run --release -p aad-engine --example geodb_groundtruth -- <file> [--units u] [--keep-linetype] [--sweep] [--edit] [--tol mm] [--min-run mm] [--rule section.field=json] [--diag] [--near x y half] [--shot png n half]`: records each object's layer, sets every layer to `0` (blocks too), builds from geometry alone and scores walls, doors and windows against the recorded layers (`*WALL*`, `*DOOR*`, `*WIND*`), then optionally measures one incremental edit. Phase 2 adds columns and beams against `*COL*` / `*BEAM*` layers, the region table, the fate of the true wall objects (kept, below threshold, turned into beams) and the true and false wall objects per region. `--rule room.isolated_penalty=0.4` sets any rule field for a run. Golden files: `crates/geodb/tests/golden/` (rewrite with `GEODB_BLESS=1` after checking a change). The engine test `a_plan_drawn_with_the_wall_commands_is_recovered_from_geometry_alone` scores Tren's own plans the same way. Numbers: [GEOMETRY-DB-RESULTS.md](GEOMETRY-DB-RESULTS.md).

## 13. Phases

| Phase | Delivers | State |
|---|---|---|
| 0 | schema, extract, R-tree, scale check, incremental update, bulk write | built, tested |
| 1 | pairs, wall runs (three types), doors, windows, rules table, review keys, `geodb.*` commands | built; accuracy on real plans below the 95 % target, see results |
| 2 | columns, beams, rooms, page regions, hatches, slab | built; wall precision on the real plan is still far below 95 % (29.8 % precision, 49.4 % recall on `Working Drawing.dwg`), see results |
| 3 | electrical and plumbing points and networks | planned |
| 4 | status-bar summary panel, `classify.set` stamping, review UI | planned |
| 5 | AI tools over the tables, model comparison | planned |
