# Geometry database: measured results (phases 0 and 1)

Measured 2026-11-08 on a Windows machine with the release build of the harness (`crates/engine/examples/geodb_groundtruth.rs`, see [GEOMETRY-DB.md](GEOMETRY-DB.md) section 12). Only numbers that were measured are given. **The 95 % wall precision and recall target was not reached on the real plans measured; the reasons and what is needed are below.**

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

- Columns, beams, slabs, rooms, hatches, electrical and plumbing (phases 2 and 3). Beams and columns are currently read as walls when they are drawn as two lines the right distance apart.
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
