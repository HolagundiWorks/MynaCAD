# Geometry database: measured results (phases 0 to 2)

Phases 0 and 1 measured 2026-11-08, phase 2 measured 2026-11-10 (section "Phase 2" at the end), on a Windows machine with the release build of the harness (`crates/engine/examples/geodb_groundtruth.rs`, see [GEOMETRY-DB.md](GEOMETRY-DB.md) section 12). Only numbers that were measured are given. **The 95 % wall precision and recall target was not reached on the real plans measured, in phase 1 or in phase 2; the reasons and what is needed are below.** The phase 1 sections describe the state before the context rules; the section "Phase 2" has the numbers after them.

## Method

Each drawing is read, every object's layer is recorded, every layer (blocks included) is set to `0`, the model is built from geometry alone, and each predicted class is scored against the recorded layers. Truth classes are layer names: walls `*WALL*`, doors `*DOOR*`, windows `*WIND*`/`*WINDOW*`/`*GLAZ*`, excluding names containing hatch, text, dimension, tag, finish, pattern, annotation, note, plaster, tile or fence. Scores are on line, polyline, arc and circle objects (entity level: a wall face line counts as one object; the harness gives an object claimed by an opening to the opening). The rules never see layers in this test. Rule defaults were not tuned on these files, with two exceptions listed under "What changed because of the measurements".

## Drawings used

| Drawing | Entities | Usable as ground truth? |
|---|---|---|
| `Desktop\Working Drawing.dwg` (inches, 26,206 entities) | plan, elevations, structure and details on one sheet | Walls and windows yes (`AR_WALLS` 3,193, `AR_WINDOWS` 990). No door layer: doors are not scored. |
| `DemoCompany\cad\D1.dwg` and `DEMO.dwg` (identical content, 9,651 entities) | measurement overlays (`MEASURE-*`), almost everything on layer `0` | **No.** No wall, door or window layer exists, so there is no truth. Only counts and timing are meaningful. |
| `DemoCompany\Demo-Villa.osp` | its drawing holds a single entity | No. |
| `DemoCompany\projects\...\model.dxf` | empty | No. |
| `C:\Temp\video\apw.dwg` (232,212 entities) | 231,678 of them on layer `0`; layers `Windows` 290, `COLUMN` 146, `Doors` 15 | Windows/doors only, weakly (see below). No wall truth. |

So wall accuracy could be measured on one real drawing only, and the "demo villa" drawings do not contain correctly layered walls.

## Walls, `Working Drawing.dwg`

| Setting | Precision | Recall | Predicted objects |
|---|---|---|---|
| Strict truth (`AR_WALLS`), confidence 0.5 (default) | **26.6 %** | **66.1 %** | 6,891 |
| Strict truth, confidence 0.7 | 27.3 % | 54.6 % | 5,525 |
| Lenient truth (walls + `*BEAM*` + `*COL*` layers), 0.5 | 46.7 % | 52.7 % | 6,891 |

Linetypes are resolved before the layers go to `0` only with `--keep-linetype`; it made no difference here.

Why it is low (from the harness listings, not assumed):

- **Precision.** The predicted objects not on `AR_WALLS` are mostly on layers that really do contain pairs of parallel lines 9 in apart: `AR_ELEVATION` 1,624 (elevation views on the same sheet), `ST_BEAMS` 1,165 and `ST_COLUMNS` 249 (a beam or column outline is geometrically a thin wall), `IN_FURNITURE` 571 (cabinet and shelf depths), layer `0` 1,075 (title blocks, details). With every layer at `0` these cannot be told apart by two-line geometry alone. The confidence does not separate them (the sweep above), because the rules score only the pair itself. Closing this needs context rules that are phase 2 (beam vs wall by linetype and by enclosing room loops, column rectangles, room closure) and a page-region split (plan versus elevation); it will not come from tuning thresholds.
- **Recall.** 769 of the missed `AR_WALLS` lines are lines (e.g. wall pieces too short or too broken to chain into a 600 mm run, and lines on the sheet's detail drawings), plus objects that are not wall geometry at all and cannot be found by this rule: dimensions 193, text 51, MText 46, points 47, proxies 43, blocks 53 (they sit on the wall layer in this file).
- The truth itself is noisy: the wall layer also holds window-grill detail drawings elsewhere on the sheet.

An engine-made plan (walls, doors and windows drawn with `wall.draw`, `wall.door`, `wall.window`, all layers set to `0`; engine test `a_plan_drawn_with_the_wall_commands_is_recovered_from_geometry_alone`): wall precision **100 %**, recall **90 %** (18 of 20 wall objects; the 2 missed are end-cap pieces of the partition), both doors and both windows found, nothing extra. This is a synthetic plan with clean geometry and says little about real drawings.

## Doors and windows

- `Working Drawing.dwg`, windows: **64.3 % precision, 81.5 % recall** on line/arc/polyline objects (804 true positives of 986 true window-layer objects; 1,251 predicted). Of 502 window features classified, 212 (42.2 %) had a member on a true window layer. Window lines are also drawn on layers other than `AR_WINDOWS` in this file, so some "false positives" are probably real windows; this was not checked object by object.
- `Working Drawing.dwg`, doors: 48 door-swing features classified but **no door layer exists, so no door accuracy was measured**. By eye on the screenshots (swing arcs with a leaf at wall openings) the swing rule hits real doors, but that is not a measurement.
- `apw.dwg`, doors: 136 door features classified (mostly blocks named like `*DOOR*`), 2 of them (1.5 %) with a member on the `Doors` layer (15 objects there); recall on the 9 line/arc objects of that layer 0 %. Windows: 94 classified, 0 on the `Windows` layer (290 objects). **These are failures**: in `apw.dwg` the objects on the `Doors` and `Windows` layers are not the symbols the rules look for (not recognised blocks or swing arcs on walls), and no wall truth exists to say whether the walls around them were found. Not investigated further.
- No 90 % door/window target was met or could be checked on a real drawing with door truth.

## Scale check

`Working Drawing.dwg` and `D1.dwg` (declared inches): the check picks `in` (score 0.35 and 0.40 against at most 0.12 for the others), median pair gap 228.6 mm. `apw.dwg` (declared inches): the check prefers `ft` (0.30 against 0.08 for `in`), median gap 228.6 mm, i.e. the wall pairs in that file are 0.75 units apart; the build returns a "the drawing says in but its wall thicknesses fit ft much better" warning and the declared unit is still what is used. Either the file is drawn in feet or its scale is off; it was not resolved. The unitless path (propose, then require confirmation) is covered by tests on a synthetic drawing in metres, not on a real unitless file.

## Timing (release build, single thread, this machine)

| Drawing | Read (DWG to drawing) | Full build: extract / scale check / classify / total | Rows | One-edit incremental |
|---|---|---|---|---|
| `D1.dwg`, 9,651 entities | 0.3 s | 12 / 92 / 86 ms / **0.19 s** | 16,919 segs, 10,102 pairs | **14 ms** |
| `Working Drawing.dwg`, 26,206 | 0.8 s | 45 / 183 / 325 ms / **0.56 s** | 70,985 segs, 23,948 pairs | **27 ms** |
| `apw.dwg`, **232,212** | 5.1 s | 290 / 553 / 1,812 ms / **2.7 s** | 472,304 segs, 367,824 prims, 62,430 pairs, 58 M candidate comparisons (no cap hit) | **620 ms** (1 edit); 1 ms when nothing changed |

The incremental figure is for moving one end of one wall line by 10 mm and updating: it re-extracts that one entity and re-derives the window around it. On `apw.dwg` the window is large because one feature spans much of the plan; the figure varies with which wall is edited (one earlier run with a wider window took 3.8 s, which is why the window was narrowed). Times are wall clock, not a profile; memory was not measured. The write of the tables into the `.osp` (`store::write_full`) is covered by tests for correctness; its time on the heavy file was **not measured**.

## What changed because of the measurements

- Pair overlap threshold lowered from the draft's 600 mm to 100 mm, with the 600 mm length test moved to the chained run (`min_run_mm`): real walls are drawn in short pieces. Wall recall on `Working Drawing.dwg` went from about 31 % to 49 % with this and to 66 % with wall end caps and jambs attributed to the wall.
- A run wholly inside a thicker run is penalised (window sill lines read as a half-brick wall); a run that is the sides of one small closed rectangle is penalised (column sections).
- A block penalty was tried (faces inside blocks): it lowered recall (49 % to 41 %) for a precision gain of one point, so its default is 0. It remains a rule parameter.
- The scale check uses a wall-length overlap; with the short pair threshold it scored coincidences between long unrelated lines.
- The wall tolerance was tried at 8, 12 and 16 mm on `Working Drawing.dwg`: recall 49.1, 49.3, 49.3 %. It stays 8.

## Tests

- `cargo test -p aad-geodb` (31 tests): extraction transforms and block depth limit, pairing (wrap at 0/180, reversed lines, ladder, 40,000-segment speed), wall types in metric and imperial, unitless proposal and confirmation, stamp beats `INSUNITS`, stair treads, short rectangles, door and window blocks, hostile numbers, incremental update equals a full rebuild after replace, erase and add, bulk write and incremental write to a project file (rows match, no `change` rows), rule edits and rejected edits, feature keys, golden file `tests/golden/room.txt`.
- `cargo test -p aad-engine --lib geodb` (7 tests): build leaves the drawing untouched; apply needs a preview and an approval, a stale preview is refused, approval is one undo step and undo restores; unitless drawings are not applied until confirmed; rule commands; hostile parameters; layer-map suggestions; the stamp and units surviving a DXF write and read; the engine-made plan above.
- `cargo test --workspace --exclude aad-winui` ran green before the final small edits (scale-check overlap, command-log line, clippy fixes); after them `cargo test -p aad-geodb -p aad-engine --lib` (390 tests) passed again. The full workspace run was not repeated after the last edits. `cargo clippy` is clean for `aad-geodb` and `aad-engine` (checked with `--all-targets`); the wasm32 check of `aad-geodb` passes. `cargo xtask layers` passes for the new crate; it reports `aad-odp` and `aad-dbconnect` as unregistered, which was already the case. The rest of `cargo xtask ci` (assets, licences, UI design system) was not run. The WinUI shell was not built or run.

## Not handled yet

- Electrical and plumbing (phase 3). (Phase 2 now has columns, beams, rooms, hatches and the slab; its limits are in the next section.)
- Walls that are not two lines (single centre lines, solid hatch strips) are dropped by decision, so they are not found.
- Walls drawn as more than two lines (cavity walls drawn as two leaves with a gap), thicknesses outside the three types (220 and 110 mm walls appear in `Working Drawing.dwg`), and splayed or curved walls.
- Doors with no swing arc and no block; windows with no interior line and no gap; double-leaf doors count as two doors; sliding doors.
- Only the active model space level is read; paper space, xrefs, dimensions, splines, ellipses and leaders are skipped (counted in `skipped`). Hatch boundaries are kept in `geo_hatch` only, and their areas ignore bulges.
- The shell shows only the command-log line `build` writes; there is no report panel, and no panel to accept or reject rows (the commands `geodb.report`, `geodb.review`, `geodb.apply` are available to the command line, scripts, control and MCP). The build is available off the UI thread (`spawn_build`) but no shell code calls it; `geodb.build` runs on the calling thread.
- `geodb.apply` does not stamp `BuildingElement` (`classify.set`), and it does not feed the take-off.
- `geo_review` is written when a `.osp` is open; with no project file, reviews live only for the session.

## What the layer mapper still lacks

`layermap` takes an explicit `{old: new}` map and moves objects (skipping locked layers, optional create and purge). Added here: `suggest: true` (keyword candidates, no change) and the `layerMapDone` mark. Still missing for a "layer mapper studio": a saved, named mapping per company or per source program; matching by more than keywords (block names, colours, linetypes, what the objects look like); a review screen with counts and a preview of the result; handling of layers that mix classes (all on `0`, which geometry classification, not name mapping, has to split); mapping of block-internal layers, linetypes and text styles; an undo-friendly report of what moved. The shell does not raise it automatically on open: a foreign drawing gets a command-line notice, `origin` in the `open` result and a refusal from `geodb.apply`.

## Other follow-ups

- Origin detection treats a drawing from an older Tren (no stamp) as foreign until it is saved once. `$ACADVER`, `$DWGCODEPAGE` and application ids are reported but not used to decide.
- The `.osp` `meta` keys `drafting_units`, `drafting_unit` and `origin` are written through a second SQLite connection to the project file (WAL, 5 s busy timeout); `aad-odp` itself was not changed.
- New drawings choose their drafting units only through `new_drawing(metric)`; the project wizard's `units` and `drawingUnits` choices are not yet written to `draftingUnits`.

## Phase 2: columns, beams, rooms, page regions, hatches, slab

Measured 2026-11-10 with the same harness (release build, same two files, default rules). What the stage does is in [GEOMETRY-DB.md](GEOMETRY-DB.md) sections 5 and 7. **Verdict: the 95 % wall target is still not met, and by a wide margin.** On `Working Drawing.dwg` wall precision went from 26.6 % to 29.8 % while wall recall fell from 66.1 % to 49.4 %; the harder choice of what counts as a wall now removes about as many true walls as false ones. Taken as one number (F1) it is 0.372 against 0.379 before, i.e. no gain. Do not read the phase 2 rules as having fixed wall accuracy.

### Walls, `Working Drawing.dwg` (strict truth `AR_WALLS`, line/arc/polyline objects, confidence 0.5)

| | Predicted objects | Precision | Recall |
|---|---|---|---|
| Phase 1 (before) | 6,891 | 26.6 % | 66.1 % |
| Phase 2 (after), default rules | 4,594 | **29.8 %** | **49.4 %** |
| Phase 2, `beam.structure` 0.6 (a structural layout alone makes a beam) | 3,269 | 31.8 % | 37.6 % |
| Phase 2, `region.gap_mm` 800 | 3,977 | 32.2 % | 46.2 % |

The parameter changes in the lower rows were measured on this one file with this one truth; they are not defaults and are not evidence that they generalise. Raising `classify_min` to 0.6 or 0.65, stronger isolated and floating penalties, a shorter `bridge_mm` and a larger `wall_hatch_bonus` each moved precision by under two points, mostly at the cost of recall.

What the context stage did to the 2,768 true wall objects (before: 1,846 reach a wall run at confidence 0.5 or more):

- 1,374 stay walls, 241 stay below the threshold, 391 became beams (structural layouts and floating pairs), 298 are also members of a column feature.
- Of the wall objects predicted after the stage, 1,227 true and 3,281 false sit in plan regions, 388 true and 1,439 false in structure regions. The false ones by layer: `AR_ELEVATION` 1,057, `ST_BEAMS` 901, `IN_FURNITURE` 435, `0` 343, `N_TEXT` 224, `ST_COLUMNS` 203, `AR_WINDOWS` 81.

Why precision did not rise further (from the listings, not assumed): the false positives are spread through the plan regions themselves. The region split removed the pairs in 13 regions it judged to be neither plan nor structure (514 wall runs lost 0.5), but elevation lines (`AR_ELEVATION`), furniture and window frames sit inside regions that are plans, so they are pairs inside a real plan and look like walls there. A structural layout also draws the walls (388 true wall objects are still walls in the structure regions), so its pairs cannot all be beams. Whether the elevation lines really lie inside the plan regions or are plan-like drawings of their own was not investigated further.

### Other classes, `Working Drawing.dwg`

| Class | Truth (layer) | Predicted objects | True positives | Precision | Recall |
|---|---|---|---|---|---|
| Columns | `ST_COLUMNS` 466 objects (all types) | 464 | 117 | 25.2 % | 25.1 % |
| Beams | `ST_BEAMS` 3,470 objects | 2,071 | 457 | 22.1 % | 13.2 % |
| Windows (phase 1: 64.3 % / 81.5 %) | `AR_WINDOWS` 986 line/arc/poly | 957 | 804 | **84.0 %** | **81.5 %** |

The window figure is as measured; the cause of the change from 64.3 % was not isolated (columns no longer enter the wall candidates, wall confidences changed, and a window needs a confident wall run). Columns: 2,856 rectangle candidates and 654 circles were found, 991 and 97 of them above the threshold; truth objects on the column layer include other entity types. Beams: 866 beam features; every beam number here comes from the structure weight, floating pairs and wall-across evidence, because the dashed rule never fired. Resolving by-layer linetypes before zeroing the layers (`--keep-linetype`) changed no number, so either the beams in this file are drawn with a continuous linetype or the linetype is not carried the way the rule reads it; that was not investigated. The 95 % target for any of these classes was not met.

### Rooms, regions and slab (no ground truth)

`Working Drawing.dwg` (one sheet with many views): 40 regions, of which 17 plan, 3 structure and 13 other (the rest have no wall runs or were not judged); 220 rooms totalling 2,000 m2 and 36 slab outlines totalling 2,700 m2; 1,374 hatches linked to a room or wall and 4,099 not (most are outside any judged wall loop). Region sizes and door counts per region are printed by the harness; two plan views are exact copies (identical counts), as the sheet repeats them. **No room count was checked against a person's count**, so these numbers show that the stage runs and closes loops, not that the rooms are right. `apw.dwg`: 24 regions, 1 plan, 4 structure, 13 other, 2 rooms (5.4 m2), 2 slab outlines. That file is almost all on layer `0` and is not a clean plan; it has no wall truth. Its `COLUMN` layer (146 objects) was matched 0 times by the column rule and its `Doors`/`Windows` layers still do not match the door/window rules (unchanged from phase 1).

The unit tests cover the mechanics on synthetic drawings: an engine-made room gives one room of 19.6 m2 (within 3 % of the clear area) and one slab; an elevation beside a plan loses its walls and the same lines alone on a sheet stay walls; a dashed pair and a floating pair become beams while the room stays one room; a column grid without doors is a structure region; a hatch in a room, one in a wall and one nowhere are linked as such.

### Timing (release build, this machine at the time of the run)

| Drawing | Read | Full build: extract / scale check / classify (of which context) / total | One-edit incremental | Nothing changed |
|---|---|---|---|---|
| `Working Drawing.dwg` | 0.9 s | 51 / 211 / 806 (313) ms / **1.08 s** | **388 ms** (phase 1: 27 ms) | 0 ms |
| `apw.dwg` | 5.8 s | 315 / 577 / 2,132 (93) ms / **3.07 s** (phase 1: 2.7 s) | **1,151 ms** (phase 1: 620 ms) | 2 ms |

The incremental time went up because the context stage looks at the whole sheet and is recomputed after every edit (that is what makes an incremental result equal a full rebuild, and the unit tests check that it does). Making it local to the region around an edit is a follow-up. The raster used 9.2 million cells on `Working Drawing.dwg` and 2.2 million on `apw.dwg`. The write of the new tables into the `.osp` was tested for correctness, its time on the heavy files was not measured.

### Tests

`cargo test -p aad-geodb -p aad-engine --lib` passes (40 + 360 tests; 9 new in `aad-geodb`, 1 new in the engine). Clippy with `-D warnings` is clean for both crates with `--all-targets`; the wasm32 check of `aad-geodb` passes. The golden file `crates/geodb/tests/golden/room.txt` was rewritten (the room now also has a room and a slab, and its walls a higher confidence because they bound the room); this was checked by eye against the numbers above. `cargo xtask ci` (assets, licences, layering, UI design system) and the full workspace test run were not repeated for this change. The WinUI shell was not built or run.

### Known limits of the phase 2 rules

- Rooms need walls that close. A wall that stops more than `bridge_mm` (1.5 m) before the next one leaves the room open and it is not found; a wall end that stops within it is closed even when it is a free-standing partition (the unit test keeps its floating pair more than 1.5 m from the walls for this reason).
- A column is any closed rectangle or circle of column size; fixtures and window panes look the same, so a rectangle needs a fill, a column layer name, a structure region or a typical size next to a wall to pass the threshold. Round columns are less specific still.
- Beams: nothing in a pair separates a beam from a wall except linetype and context; with continuous beams in a structural layout the region weight alone is deliberately below the threshold, so many of them stay walls.
- Plan / elevation: a plan needs at least one door feature. A plan drawn without doors on a sheet that also has a plan with doors is judged "other" and loses wall confidence. Views closer than about 1.5 m are one region.
- Hatch assignment uses the centre of the first boundary loop; an L-shaped hatch whose centre falls outside it is not linked. The slab is the outline of the filled shape, not a separate structural slab.
- The context stage reads only walls the first stage found; a wall missed there is also missing from the rooms.
