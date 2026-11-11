# Levels and sheet sets

**Status: the engine side of all five phases is done. The WinUI shell has level tabs, drawing tabs
above the drawing, a Trace settings form, and a Navigator for Compose Views, Master Layouts and Publisher Sets
(see "WinUI shell status" and "Compose and publish").** The annotation tool strip, a viewport Level /
Sheet set / Traces editor, a Show-for dropdown in the Layer manager, the new-drawing option, the
egui tabs are not built. Only what the "Phase N status" sections list is
implemented and tested.
Sections and elevations (Phase 6) are a design only and are not built: see "Sections and elevations".

Summary: keep one drawing file, but split its single model space into named **levels** (each its own
entity store at a shared origin) and add **sheet sets** (discipline annotation spaces that reference
levels read-only). Every phase is useful and testable alone, starting with levels.

## Requirements

- Levels replace the single Model tab. Each level (Basement, Ground, First, Terrace, ...) is a model
  space for drafting and design. Level tabs sit at the bottom, where the Model tab is now.
- Any level can be **traced** under any other, several at once.
- Sheet sets are tabs above the drawing (Working drawing, Electrical, Plumbing, Interior, False ceiling).
  They hold annotation and dimensions, reference levels, and never change design geometry.
- Paper layouts stay. Each viewport says which level, section or elevation and which sheet set it shows.
- **Sections and elevations** (added 2026-10-27) are a second kind of model space, drafted by hand
  in 2D, each tied to a marker on a level, with read-only projection guides worked out from the
  levels. See [Sections and elevations](#sections-and-elevations-phase-6-planned).

## Concepts

| Term | Meaning |
|---|---|
| Level | Named model space with its own objects. All levels share one world origin and one layer table. Walls, doors, rooms, columns, stairs, take-off and the parametric graph act on the active level only. Elevation and height come from the existing `levels` / `floors` data |
| Site origin | Shared world XY base (`Drawing::site_origin`, project key `siteOrigin`, command `site.origin`). Default `(0, 0)`. Not a per-level offset. The WinUI host draws a world-space SITE mark (with the UCS Icon toggle); egui axes pass through the same point |
| Trace | Read-only faded display of another level under the active one. Per trace: fade (0-90 %), colour (grey or true), order (below or above), object-snap switch, optional layer list. Never selectable or editable. At most 8 per level; a level may not trace itself |
| Sheet set | Named discipline with one annotation store **per level** (text, dimensions, leaders, tags, symbols, hatches). While active, the level is shown locked and only annotation is drawn |
| Sheet view | (sheet set, level): that level's geometry (locked, filtered by the set's layer visibility) plus the set's annotation for it |
| Design mode | No sheet set picked. The default; where drafting happens |
| View space | A section or elevation: its own store, tied to a marker on a level, drafted by hand, with projection guides (Phase 6, planned) |
| Compose View | Named display recipe for paper (ArchiCAD View Map analogue): level, optional sheet set, scale, plot style, traces and optional model window. Not a DXF `VIEW` and not a Phase-6 view space. Commands `compose.view.*` |
| Master Layout | Shared paper size, orientation and title-block / sheet-template chrome applied to many layouts. Commands `master.*` |
| Publisher Set | Named list of layouts with output format (`pdf` or `dwg`), a one-file switch and a folder. `publisher.check` reports whether each sheet's paper contents fit the printable area. `publisher.publish` plots or writes the set and, by default, records the take-off into the open `.osp` |
| Layout | Paper. Optional `master`; each viewport may name a level / sheet set / traces and/or a Compose View |

## Compose and publish (engine)

ArchiCAD separates Stories → Views → Layout Book → Publisher. MynaCAD maps that as Levels → Compose Views → Layouts (with Masters) → Publisher Sets. The WinUI **Navigator** window (File > Publish, or Architecture > Sheets) shows the three trees and the one-button **Publish and record take-off**.

| Command family | Purpose |
|---|---|
| `compose.view.save/list/set/delete/apply` | Save a display recipe; apply it to a viewport (`viewport.set` + stamp `compose_view`). `compose.view.set` writes the same recipe back onto every viewport that already uses that view |
| `master.new/list/set/delete/apply` | Create A0–A4 masters; apply copies page setup onto a layout and runs `sheet.titleblock` (and optional `sheet.template.apply`) |
| `publisher.check` | For each layout in the set: `fits` is false when paper-space contents are larger than the printable area. Nothing is written |
| `publisher.new/list/set/delete/publish` | Named sets. PDF writes one file per layout, or one multipage PDF when `singleFile` is set. DWG writes one file per layout, or one DWG of those layouts when `singleFile` is set. `takeoff` (default true) calls `takeoff.record` with `annotate: false`; a drawing that is not an `.osp` still publishes and reports that the take-off was not recorded |

Persistence: project key `compose1` (synced on DXF/DWG write in `levels_file::to_file`, restored in `from_file`). Layout→master and viewport→compose-view links ride in the same JSON. Limits: 256 compose views, 32 masters, 32 publisher sets, 256 layouts per set. Verified by `aad-doc` compose unit tests and `aad-engine` `cmd::compose` tests.

Annotation lives in its own store, not on layers, so two disciplines never collide, take-off never
counts annotation, and deleting a sheet set never touches the design.

## Sections and elevations (Phase 6, planned)

Sections and elevations are a second kind of model space (a **view space**), drafted by hand in 2D
like a level, but each is tied to a marker on a level and shows **projection guides** computed from
the plans. Nothing is generated from a 3D model: guides are 2D projections of plan geometry plus
heights already stored in the drawing, so the 2D-only scope holds.

### View space

- Its own store and tab (`Section A-A`, `Elevation North`), drafted with the normal tools.
- Fixed coordinates: X is the distance along the marker line from its first point; Y is height in
  drawing units from datum 0. A level at +3.150 m is at Y = 3150 (mm drawings) in every section.
- Tied to a **marker** on a level. Today `section.marker` and `elevation.marker` (`cmd/symbols.rs`)
  draw loose lines, circles and text with no link. They become one block insert each
  (`AAD-SECTION`, `AAD-ELEVATION`) carrying the view id as extension data, so moving or rotating
  the marker updates the view. `section.link {handle | window}` adopts an existing loose marker.
- A **depth** (how far past the line it looks, > 0) and the levels it covers (default all).

```text
SpaceId(u32)                      shares the id counter with LevelId; never reused
ViewSpace { id, kind Section|Elevation, name (1..64, unique among views), level: LevelId (the marker's plan),
            marker: Option<Handle>, line: (Vec2, Vec2) plan coords, look Left|Right, depth > 0,
            covers: Option<Vec<LevelId>>, traces[<=8] (other view spaces only), view, entities (skip) }
Drawing += views (<=128)
EditTarget += View(SpaceId); SheetSet.annotations is keyed by space (level or view), not LevelId only
```

`switch_level` generalises to the edit-target switch (Phase 3 introduces `EditTarget`); the swap
invariant stays in one function. `model_stores` / `model_stores_mut` include view stores, so purge,
audit, layer rename and the other drawing-wide walks of Phase 1 cover them with no new call sites.

### Projection guides

Read-only lines computed from the levels, drawn through the existing trace path
(`crates/render/src/trace.rs`, quiet colour, `VIEWPORT_CONTENT` handle so never selectable),
snappable, and recomputed when a covered level's store changes (`Arc::ptr_eq`, the same cache rule
as traces). Guides are never saved; they are recomputed on open. What the user drafts is never moved.

| Guide | Source | Section | Elevation |
|---|---|---|---|
| Floor and top-of-storey lines, labelled with the level name | each level's `elevation`, `height` | yes | yes |
| Wall faces | cut line crossing wall lines on each covered level | cut walls, both faces | outer faces seen within depth |
| Door and window jambs | opening inserts (`walls.rs`) within depth | yes | yes |
| Sill and head lines | opening attributes `OSILL`, `OHEIGHT`, `OHEAD` | yes | yes |
| Columns, stair start and end | column and stair objects crossing or seen | yes | seen only |

Guide maths is planned for `crates/engine/src/sectionguide.rs` (the file does not exist yet): plain 2D line/segment intersection and
projection onto the marker line. Hostile-input rules apply: zero-length line, depth 0 or negative,
NaN or infinite heights, more than 100 000 guide lines (capped, with a note), nested blocks bounded by
`MAX_BLOCK_DEPTH`. Guides know only MynaCAD walls, openings, columns and stairs (or objects carrying
their attributes); plain lines give none. Roofs, sloped slabs and curved walls in elevation are
drafted by hand.

### Commands (`cmd/views.rs`)

| Command | Purpose |
|---|---|
| `section.new {p1, p2, look?, depth?, label?, level?}` | Place a linked section marker on a level and create its view space (does not switch to it from JSON) |
| `elevation.new {at, angle, depth?, name?, level?}` | Same for an elevation |
| `view.list`, `view.set {view, name?, depth?, covers?, look?}`, `view.delete {view, force?}`, `view.activate {view}` | Manage view spaces; each is one undo step; deleting the marker leaves the view with `marker: None` and a note |
| `view.trace` | Traces between view spaces (levels are not offered: plan and section coordinates differ; guides do that job) |
| `section.link {handle}` | Adopt an existing loose marker |
| `section.draft {view, slabThickness?}` | Turn the current guides into ordinary editable objects in one undo step: cut walls as closed outlines, slabs as bands, openings as outlines between sill and head. Running again adds only what is new; never moves drafted objects |
| `section.check {view}` | List guides that no longer meet drafted geometry (for example a wall moved 150 mm), as a report; changes nothing |
| `section.levelmarks {view}` | Place `level.mark` symbols at each floor line, named after the levels |

### UI, sheet sets, layouts, files

- **Tabs:** a second group in the bottom row between levels and layouts
  (`Ground | First · Section A-A | Elevation North · Title plate | Layout1`); a group with more than
  about six tabs collapses into a dropdown chip (`Sections ▾`). Double-click on a marker opens its
  view; right-click on a view tab offers Show marker. `chrome_json` gains `views`.
- **Sheet sets:** annotation per space, so Working drawing holds Section A-A's dimensions and
  Electrical the switch heights on Elevation North.
- **Layouts:** `Viewport.level` generalises to a space id (level or view). The marker head's sheet
  field is filled from the layout whose viewport shows that view, as `elevation.marker` takes `sheet`
  by hand today.
- **Files:** view stores as blocks `ODRAFT$VIEW$<id>` with keep-alive INSERTs on `ODRAFT-STORES`;
  view records (kind, marker handle, line, look, depth, covers, traces) in `levels2` (bump its
  version; older readers already fall back to one level on an unknown version). The marker stays an
  ordinary block in other CAD programs.

### Done when

Moving a wall on Ground moves its guide in Section A-A without touching drafted lines,
`section.check` reports the gap, the drawing round-trips through DXF and DWG with views, markers and
links intact, and the hostile-input tests pass.

## Data model (`crates/doc`, L1)

`Drawing.model` always holds the one store being edited; every other store waits in its slot. Commands
keep writing to `d.model`, so the existing call sites do not change; only drawing-wide code (purge,
audit, layer rename, extents) must learn about the other stores.

```text
LevelId(u32), SheetSetId(u32)        stable, never reused
Level     { id, name (1..64, unique), tab_order, elevation, height, traces[<=8], view, entities (skip) }
Trace     { level, fade 0..=90, colour Grey|TrueColour, order Below|Above, snap, layers? }
SheetSet  { id, name, tab_order, space?, layer_prefix, dim_style?, text_style?, annotations: LevelId -> store (skip) }
EditTarget = Level(LevelId) | Sheet(SheetSetId, LevelId)
Drawing  += levels (<=64), sheet_sets (<=32), target
```

Methods: `switch_target(EditTarget)` (the only place the swap invariant lives; used by commands, undo and
file load), `store(target)`, `all_stores()`, `reference_stores()`. Handles stay drawing-wide so a
sheet-set dimension can reference a wall handle on its level. Because levels live inside `Drawing`,
undo restores them, including the active tab. `Viewport` gains `level?` (none = first level),
`sheet_set?` and `traces`. All limits are checked on every input path (JSON, DXF metadata).

## Commands (`cmd/levels.rs`, `cmd/sheetset.rs`)

| Command | Purpose |
|---|---|
| `level.list`, `level.new {name, elevation?, height?, after?, copyFrom?}`, `level.set`, `level.delete` (refused for the last level or one in use, unless `force`), `level.activate` | Manage levels; each is one undo step |
| `level.trace {level?, add?, remove?, fade?, colour?, order?, snap?, layers?}` | Edit a level's traces |
| `level.copyto {levels\|level, selection?, from?, to?}`, `level.moveto {level, create?, handles?, from?, to?}` | Copy or move the selection between levels. Optional `from` / `to` shift geometry so a picked origin matches a point (default `to`: site origin). `level.moveto` moves objects from whichever level they are on, keeps every property and mark, and with `create` makes the target level first (a drawing with one implicit level gets its first explicit level named after the current floor, else `Level 1`); a refused call creates no level |
| `sheetset.list / .new / .set / .delete`, `sheetset.activate {set\|null}` | Manage sheet sets; `new` with `preset` creates the five standard ones |
| `viewport.set` (extended) `{handle, level?, sheetSet?, traces?}` | What a layout viewport shows |
| `level.import / level.export {path, level}`; `level.fromfloors`; `project.merge` | Migration (below) |

Rules: in a sheet view the `protect.rs` check treats reference stores as locked and names the level;
draw commands in a sheet view put objects on the set's layers (`E-DIM`, `E-TEXT`, ...); selection,
`qselect`, `filter`, `selectsimilar` see only the edit target; inquiry commands may read traces and
the referenced level through snaps; programmatic calls never open dialogs.

## UI (WinUI shell stays thin)

- **Bottom tabs:** level tabs in tab order, a `+` (`level.new`), then the level tools. Paper layouts are not tabs. A print block (`printblock`) names a region by a boundary drawn on the model, the way a block is named. Its description is the level and the tab (the sheet set, or Design) and follows those names when they are renamed. The description is stored on the print block when it is listed. The boundary determines which objects are covered. Nothing is frozen and the objects are not converted into a block. The project map, the layout, the nameplate and the sheet PDF are composed in MynaOps. Right
  click on a level: Rename, Elevation and height, Copy level, Delete, Trace over current level. A
  **Trace** flyout lists the other levels (tick, fade slider, grey/true colour, below/above, snap).
- **Drawing tabs** sit above the drawing: Design, then each sheet set. With no sheet set, a `+` tab
  adds Working drawing, Electrical, Plumbing, Interior and False ceiling. Choosing one keeps the
  current level. The left column keeps the draw, modify and annotate strip. The discipline tools
  under it follow the active drawing tab: Design and Working drawing show openings, rooms and
  structure; Electrical, Plumbing, Interior and False ceiling show that tab's symbols. A renamed tab
  keeps those tools when its layer prefix is still `E-`, `P-`, `I-` or `FC-`.
- Layer manager "Show for" filter (All / This level / This sheet set); viewport properties gain Level,
  Sheet set and Traces; new-drawing and `project.setup` offer levels; Ctrl+PageUp/PageDown change
  level, Ctrl+Shift+PageUp/PageDown change sheet set. Tab data comes from `chrome_json`
  (`levels`, `layouts`, `sheetSets`). The egui shell follows later.

## Rendering, snaps and selection

Phase 2 changes `aad_render` and the host canvas; **coordinate the order with the rendering work
before starting it.**

- One cached mesh per store. Draw order: traces Below, the referenced level (sheet view), the edit
  target, traces Above. Grey maps colours to the theme's quiet colour; fade sets alpha; trace
  lineweights draw thinnest.
- A trace mesh rebuilds only when its level's store changes (`Arc::ptr_eq`), so tab switches and pans
  stay on the cached path of [PERFORMANCE.md](PERFORMANCE.md). `lod_for_view` applies to traces.
- Layout viewports read `d.store(viewport.level)` plus the set's annotation and the viewport's traces;
  frozen layers and colour overrides apply to all.
- Object snaps use the edit target, the referenced level in a sheet view, and traces with `snap` on
  (markers in the quiet colour). Select and grips act on the edit target only. Zoom Extents uses the
  edit target plus visible traces.
- Budget: with three traces of 25 000 objects each, pan and zoom stay cached and a test shows that a
  level switch does not rebuild unchanged meshes.

## DXF and DWG round-trip

The file stays a normal DXF. DWG uses the same bridge.

| What | Written as |
|---|---|
| First level in tab order | Model space, whatever tab was active on save |
| Other levels | Block `ODRAFT$LVL$<id>` |
| Sheet-set annotation for a level | Block `ODRAFT$ANN$<set id>$<level id>` |
| Keep-alive | One INSERT of each block in model space on layer `ODRAFT-STORES` (frozen, not plotted) so `PURGE` keeps them |
| Level, trace, sheet-set records | JSON under `levels2` in `AAD_PROJECT` (version, ids, names, order, elevations, traces, sets, active target) |
| Section and elevation spaces (Phase 6, planned) | Blocks `ODRAFT$VIEW$<id>`; records in `levels2` |
| Viewport level, set, traces | Extension data `AAD_VP` on the VIEWPORT |

Reading: the blocks named in `levels2` are read back into stores and the keep-alive INSERTs and layer
are hidden from the user. Missing or damaged metadata never fails the open: the drawing loads as one
level and a warning is reported; a record naming a missing block gives an empty level and a warning.
Handles inside level blocks keep their values. Counts, ids and nesting (`MAX_BLOCK_DEPTH`) are
bounded, with a `hostile_dxf_does_not_panic`-style test. Known limit: in another CAD program a
viewport set to a non-first level still shows model space; `level.export` gives one DXF per level.

## Migration

Nothing changes in an existing drawing until a second level or a sheet set is added; such files save
as before. They open as one level named after `floor.current`, else "Level 1". `floors` / `levels`
project data stay the source of names and elevations (`level.new` and `level.set` keep them in step).
Floor projects (one file per floor with Xrefs) stay supported through `level.import`, `level.export`
and `project.merge`. Drawing spaces keep working; a sheet set points at one. Take-off rows in the project file
use the drawing slot in each id; single-level rows keep their ids. Viewports without level data
show the first level.

## Decisions (from the plan's open questions)

1. Levels in one file are the default for new projects; floor projects remain an import/export path.
2. Sheet-set annotation is stored per level.
3. The rail lists Design plus the five presets on every new drawing.
4. Traces are saved with the level.
5. Open: sections and elevations use guides plus an on-demand `section.draft`; drafted lines do not
   follow plan changes automatically (assumed until the owner decides otherwise).

## Roadmap

The plan names six phases (sections and elevations were added on 2026-10-27 as a sixth phase, after the five built ones); the split
below follows the plan (Phase 1 carries the file format and drawing-wide audit; Phase 2 is the
rendering work). Each phase ends with `cargo xtask ci` and its named tests, and updates
[status.md](status.md) in the same change.

| Phase | Scope | Done when |
|---|---|---|
| 1. Levels core | `doc` types, `switch_target`, `levels.rs` commands (list/new/set/delete/activate/copyto/moveto/fromfloors), classification of every `.model` use as edit-target or drawing-wide, drawing-wide commands use `all_stores()`, DXF/DWG round-trip with `levels2`, hostile-input tests | A two-level drawing saves, reopens and undoes identically; purge, audit, layer delete and extents see both levels; old files load as one level unchanged |
| 2. Traces and level tabs | `level.trace`, trace rendering and cache, snaps, bottom level tabs and Trace flyout, `chrome_json` data | Three 25 000-object traces keep pans cached; a tab switch rebuilds no unchanged mesh |
| 3. Sheet sets | `SheetSet`, per-level annotation stores, `sheetset.*`, `protect.rs` reference locking, annotation layer prefixes, drawing tabs above the drawing, annotation tool strip | In a sheet view a level object edit is refused naming the level; annotation of two sets on one level does not collide; presets create their spaces |
| 4. Layouts and exchange | `Viewport.level/sheet_set/traces`, `viewport.set`, layout tessellation per store, `AAD_VP` data, Export for exchange | One layout holds the Electrical and Plumbing First-floor plans side by side and round-trips |
| 5. Migration and polish (engine done; shell UI and egui not) | `level.import/export`, `project.merge`, take-off level ids, layer "Show for" filter, shortcuts, new-drawing and `project.setup` flow, egui tabs, memory load test (20 levels) | A floor project merges into one file in one undo step; docs and status complete |
| 6. Sections and elevations (planned, not built) | View spaces, linked markers, `section.*` / `view.*` commands, projection guides (`sectionguide.rs`), view tab group, per-space annotation, `ODRAFT$VIEW$` round-trip | See [Done when](#done-when) above |

## Phase 1 status: done, with the limits below

Implemented and tested (`crates/engine/tests/levels.rs`, unit tests in `crates/io/src/levels_file.rs`):

- `crates/doc/src/levels.rs`: `LevelId`, `Level`, `Trace` (data only, not drawn yet), and on
  `Drawing` `levels`, `active_level`, `switch_level`, `add_level`, `rename_level`, `remove_level`,
  `move_level`, `model_stores` / `model_stores_mut`, `level_store`. A drawing with no levels is one
  implicit level and is unchanged until the first `level.new`.
- Commands, each one undo step with count, name and parameter limits (64 levels, unique names):
  `level.list`, `level.new` (the first call names the existing level after the current floor, else
  `Level 1`, or `first`), `level.set` (name, elevation, height, order; a rename also renames the
  matching entry of the floor list and the active floor, and updates title-block `FLOOR` fields),
  `level.delete` (`force` for a level with objects or traces; the last level stays),
  `level.activate` (refused while the block editor is open; clears the selection), `level.moveto`,
  `level.copyto` (new handles, same coordinates; layer locks are honoured on a move) and
  `level.fromfloors` (an empty level per floor of the floor list, elevation from the floor heights,
  the existing objects stay on the first floor).
- **File format** (`crates/io/src/levels_file.rs`): with two or more levels, DXF and DWG store the
  first level as model space, the others as blocks `ODRAFT$LVL$<id>` kept alive by frozen,
  non-plotting INSERTs on layer `ODRAFT-STORES`, and the level records under `levels2` in the
  project data. Reading restores levels, traces, the active level and handles, and hides the
  plumbing. Damaged metadata (bad version, duplicate or zero ids, duplicate or empty names, an
  unknown active id, bad traces, more than 64 levels, negative height) opens the file as one level
  and never fails with a note on the command line; a missing level block gives an empty level and a note. One level or none writes the same
  bytes as before.
- Drawing-wide readers and writers cover every level: `PURGE` blocks, layer in-use check, layer
  rename and merge, `AUDIT` (finds and repairs errors on any level), data-extraction and compare
  walks, text styles in use, unsaved-object counts, `Drawing::entity` and `entity_count`.
- Objects on an inactive level are found, and `Drawing::modify_entity` (so the take-off, classification
  and layer-mapper code) can change them by handle; the interactive edit commands (`move` and similar)
  still act on the active level and the selection only.

Classification of the other `.model` users: they stay on the **active level** by design, because
the result belongs to the level being worked on. These are `assoc` (dimension associativity),
permit title-block tools (`bplt`), legends, the door and window schedule block counts, the WinUI Blocks panel reference list, and the finite-value check. A later
phase may widen the schedule and legend to the whole building.

Damaged or incomplete level data opens the file as one level (or a level empty) and the command line says so (`levelsNote`, set by the reader and reported by `open`).

Known limits: the viewer shows the active level only (no tabs, traces or sheet sets yet, Phases 2 and 3); in
another CAD program a level other than the first is a block definition.

## Phase 2 status: traces implemented, shell UI not built

Implemented and tested:

- `level.trace {level?, add?, remove?, set?, fade?, colour?, order?, snap?, layers?}`: a level traces
  up to 8 others, each with its own fade (0 to 90), grey or true colour, below or above, snap switch
  and optional layer list. All changes are applied to a scratch copy first, so a refused change (the
  cap, a bad level, a bad value) leaves the drawing as it was. A level cannot trace itself; deleting
  a level removes the traces of it. `level.next` / `level.prev` (Ctrl+PageDown / Ctrl+PageUp) cycle
  through the levels. Tests: `crates/engine/tests/levels.rs`.
- Rendering (`crates/render/src/trace.rs`, `build_with_traces`): each trace is a display list of the
  traced level's visible objects, greyed or kept in colour, blended towards the background by the
  fade, drawn at the thinnest weight, with wipeouts dropped and `VIEWPORT_CONTENT` as the handle so
  selection never matches it. They are merged Below, edit target, Above into one ordinary list, so
  the GPU mesh needs no change. The fade bakes in the background, so the host rebuilds the list on a
  theme change. Tests: `crates/render/tests/level_traces.rs`.
- Host (`aad-host`): model space of a level with traces is built with its traces, the incremental
  `rebuild` is skipped for such levels, and a pan stays on the cached list and mesh. `chrome_json`
  carries `levels` (`id`, `name`, `active`, `traced`, `elevation`) for the status-bar tabs.
- Snaps: a trace with `snap` on offers its snap points (`crates/engine/tests/level_trace_snap.rs`);
  traced objects are never picked or selected.
- Measured (release build, three traces of 25 000 lines each): a model-space build takes about 47 ms
  against 4 ms without traces. It is paid when the drawing changes or the zoom band changes, not on
  a pan. Probe: `cargo test --release -p aad-render --test level_traces -- --ignored --nocapture`.

Not built yet (so the Phase 2 done-when is only partly met):

- The Trace settings are a form opened from the level tab menu (see "WinUI shell status"), not a
  flyout beside the tabs; a per-layer picker is not built (layers are typed as names).
- The egui canvas does not draw traces.
- No per-store display-list or mesh cache: every rebuild re-tessellates every trace, and switching
  level tabs rebuilds everything. The plan's budget that a tab switch rebuilds no unchanged mesh
  needs that cache.
- Trace snaps scan the traced level linearly (no spatial index for inactive levels); snap markers
  on a trace are not drawn in the quiet colour yet; traced layers' colour overrides and the
  per-viewport use of traces belong to Phase 4.

## Phase 3 status: sheet sets implemented in the engine, shell UI not built

Implemented and tested (`crates/engine/tests/sheetsets.rs`, `level_trace_snap.rs`,
`crates/render/tests/level_traces.rs`, unit tests in `crates/io/src/levels_file.rs` and
`crates/engine/src/cmd/sheetset.rs`):

- `crates/doc/src/sheetset.rs`: `SheetSet`, `EditTarget`, and on `Drawing` `sheet_sets`,
  `active_sheet`, `target`, `switch_target` (the one place stores are swapped), `enter_sheet_view`,
  `leave_sheet_view`, `reference_store`, `annotation_store`. In a sheet view `model` holds the sheet
  set's annotation of the active level and the level's own objects wait in the level's slot.
  Switching level keeps the sheet set. Deleting a level or a sheet set removes its annotation.
  Limits: 32 sheet sets, 64-character unique names, layer prefix up to 8 characters.
- Commands: `sheetset.list`, `sheetset.new` (one set, or `preset: true` for Working drawing `W-`,
  Electrical `E-`, Plumbing `P-`, Interior `I-`, False ceiling `FC-`), `sheetset.set`,
  `sheetset.delete` (`force` for a set that holds annotation), `sheetset.activate {set|null}`,
  `sheetset.next` / `sheetset.prev` (Ctrl+Shift+PageDown / PageUp, cycling Design and the sets).
  Entering a sheet view on a drawing with one implicit level makes that level explicit first.
- Layer routing: after any command in a sheet view, new objects not already on one of the set's
  layers move to `<prefix>DIM`, `TEXT`, `LEADER`, `HATCH` or `ANNO` by kind (created on first use),
  whatever layer was current.
- Protection: `refuse_protected` refuses a command, naming the level and the sheet set, if it
  changed the level shown for reference (layer, audit, purge, `level.*` and `sheetset.*` commands
  are exempt). `level.moveto` / `level.copyto` are refused in a sheet view.
- The level is drawn lightly faded (35 percent, true colour) under the annotation, not selectable
  (`build_with_traces`); traces still apply. Object snaps use the level's geometry in a sheet view,
  and dimensions attach to it and follow it when the level is edited in Design (the annotation
  stores are searched and updated by `assoc`).
- Drawing-wide commands see the annotation of every sheet set (purge, layer in-use, rename and
  merge, audit finds and repairs it, extraction).
- File format: a sheet set's annotation of a level is a block `ODRAFT$ANN$<set>$<level>`, the set
  records and the active sheet view ride in `levels2`. A drawing with sheet sets and one implicit
  level round-trips as well. Damaged set records open the file as plain design with a note.
- `chrome_json` carries `sheetSets` (`id`, `name`, `active`, `layerPrefix`) and `designMode` for the
  drawing tabs.

Not built yet:

- **An annotation-only tool strip in a sheet view.** The left discipline tools follow the active
  drawing tab (see "WinUI shell status"); they are the place tools for that tab, not a separate
  annotation strip. The draw, modify and annotate strip stays on every tab.
- A new drawing does not create the five presets on its own (decision 3 asks for it in the UI).
- A sheet set's `space`, `dimStyle` and `textStyle` are stored, saved and shown, but not applied on
  entering the set (no layer visibility from a drawing space, no current styles yet).
- Take-off is unchanged: it acts on the store being edited, so it never counts annotation in
  Design; the Layer manager "Show for" filter and viewport binding are Phases 5 and 4.

## Phase 4 status: viewports and exchange export implemented in the engine, shell UI not built

Implemented and tested (`crates/engine/tests/viewport_levels.rs`, `exchange.rs`,
`crates/render/tests/level_traces.rs`, unit tests in `crates/io/src/dxf_ext.rs` and `levels_file.rs`):

- A layout viewport carries `level`, `sheet_set` and `traces` (`aad_doc::Viewport`). `viewport.set`
  takes `level` (id or name, null = the first level), `sheetSet` (null = plain geometry) and
  `traces` (`[{level, fade?, colour?, order?, snap?, layers?}]`, null clears); `viewport.add` takes
  the same. All changes are checked first: at most 8 traces, each level once, a viewport cannot
  trace the level it shows, a locked viewport keeps its binding until unlocked, and a drawing with
  one level cannot bind (it can always clear). `viewport.list` / `viewport.set` return a `binding`.
- Rendering: a viewport draws, in order, its Below traces, the level it shows, that level's
  annotation for its sheet set, then its Above traces. Traces are greyed and faded towards white
  paper. An unbound viewport shows the first level (what every older file shows), so two viewports
  on one sheet can show the Electrical and the Plumbing plan of the same floor. The viewport
  tessellation cache is keyed by the identity of every store it draws, so a binding change shows at
  once. Plots use the same path.
- Entering a viewport (`mspace`) makes the level and sheet view it shows the one edits go to; an
  unbound viewport edits the first level. A drawing without levels is not touched.
- File format: the binding is a `VPBIND` group in the viewport's AAD xdata. A record that does not
  parse, is over 64 KB, is not a JSON object, or has a trace fade over 90 percent is ignored; traces
  are capped at 8.
- `exchange.export {folder, sheetSets?, overwrite?}` (File > Export for Exchange) writes plain
  single-level DXF files: `NN_<Level>.dxf` for each level and `NN_<Level>__<SheetSet>.dxf` for each
  sheet set that has annotation on it (the geometry with the annotation on top). The files carry no
  level or sheet-set data and no layout content, file names are made safe, an existing file is
  never replaced without `overwrite`, and nothing is written if any file cannot be built. The
  drawing is not changed.

Not built yet:

- The shell controls: a Level / Sheet set / Traces editor on the viewport properties, and a menu
  item for Export for Exchange (the command is in the registry and has its menu path).
- A sheet set's `space` (layer visibility) is not applied to a viewport; every layer that is on
  shows.
- A layout viewport's content is not an editable preview of the sheet in other CAD programs: there
  a viewport bound to another level shows model space (the first level). Use the exchange files.
- Exchange files carry no layouts; a bound layout sheet is exported as a PDF with `plot`.

## Phase 5 status: migration and polish implemented in the engine, shell UI and some items not built

Implemented and tested (`crates/engine/tests/migrate.rs`, `levels.rs`):

- `level.import {path, name?, elevation?, height?, after?, sheetSet?, level?, ignoreUnits?}`: reads a
  DXF or DWG through the same reader as Open (flattened to 2D; only DXF is covered by tests) and adds its model space as a new level, or, with `sheetSet`, as that
  sheet set's annotation of a level. Handles are fresh, dimension associations are kept for objects
  that came along, the blocks a level uses and the layers and styles it needs are copied (a name this
  drawing already has keeps this drawing's definition). Units are never converted: a file in other
  units than a non-blank drawing is refused unless `ignoreUnits` is given, and a blank drawing takes
  the first file's unit and the file becomes its first level. All changes are made on a copy, so a
  failure changes nothing, and a success is one undo step.
- `level.export {level?, path, overwrite?}`: one level as a plain single-level DXF; an existing file
  is never replaced without `overwrite`.
- `project.merge {path?, annotation?, ignoreUnits?}`: every floor of a floor project (a folder or
  its `project.aad.json`; default the project the drawing belongs to) becomes a level with the
  floor's elevation and height (the manifest's metres or feet converted to drawing units). Each
  discipline layout becomes the sheet set's annotation of that floor when it holds only annotation
  (text, dimensions, leaders, hatches), a level of its own after its floor when it holds geometry, and
  is skipped when empty or missing, with a reason. The first floor's floor list and project details
  are kept when the drawing has none. A drawing with Xrefs (the project's main drawing) is refused.
- `level.setup {floors, sheetSets?}`: the new-drawing flow. A list of floors or `{count, basements?,
  terrace?}` becomes a level per floor (basements below zero), the matching floor list, the five sheet
  set presets, and the ground floor active; what is already drawn stays on the ground floor. One undo
  step; refused when the drawing has levels.
- `layer` (the layer list) takes `showFor: all | level | sheetSet` and marks each layer `onLevel` and
  `onSheetSet`, with the active level and sheet set under `scope`: the data for the Layer manager's
  "Show for" filter.
- Load: 20 levels of 25 000 objects each (500 000 objects, release build) take about 290 MB; 100
  level switches with their undo snapshots add about 4 MB (the stores are shared); saving takes
  about 1.2 s (66 MB DXF) and opening about 2.2 s. Probe: `cargo test --release -p aad-engine --test
  levels -- --ignored --nocapture`.

Not built, and why:

- **Take-off rows do not carry a level id.** The record format (`(kind << 56) | (slot <<
  32) | n`, [PROJECT-FILE.md](PROJECT-FILE.md) / MynaOps `docs/PROJECT-FILE.md`) is a contract with MynaOps; giving each level
  its own slot or header changes what that program reads, so it needs agreement there first.
  Take-off acts on the store being edited, so a recording covers one level at a time.
- The shell: the Layer manager dropdown, the new-drawing dialog's "Levels in this drawing" option
  and the viewport editor. The level tabs and the drawing tabs are built (see "WinUI shell status").
- egui tabs (the egui shell is the optional one).
- `project.merge` merges into a new or ordinary drawing; there is no reverse (levels back out into a
  floor project), only `level.export` per level and `exchange.export`.

## WinUI shell status: level tabs and drawing tabs

Built (`apps/aad-winui/LevelBars.cs`, `ComposeNavigator.cs`, `MainWindow.xaml`; the chrome JSON from `aad-host` already
carried `levels`, `sheetSets` and `designMode`):

- **Navigator** (`ComposeNavigator.cs`), opened from File > Publish or Architecture > Sheets. Three trees: Views, Masters, Publisher. Check fit calls `publisher.check`. **Publish and record take-off** calls `publisher.publish` with `takeoff: true`. Not exercised in the running window in this change; the commands it calls are covered by engine tests.

- **Level tabs** in a bar under the drawing (a row of its own, because the canvas is a native child
  window that XAML cannot overlay). One tab per level, the active one selected, a half-filled circle
  on a level traced under the active one, the elevation in the tooltip. A click runs
  `level.activate`. The bar is always present; a drawing with one implicit level shows it as an active tab
  named `Level 1`, with the add button beside it.
- **Tab menu** (right-click, opens upward): *Rename, elevation and height...* (the engine form
  `level.set`, prefilled with the level), *Trace over the current level* (a tick that runs
  `level.trace` add or remove with the default settings), *Trace settings...* (the engine form
  `level.trace`: fade 0 to 90, colour grey or true, drawn below or above, snap, and the layers to
  show typed as names separated by commas, `*` for all; prefilled with the trace's current
  values, which the chrome JSON carries per level as `trace`), *Delete level...* (the engine form
  `level.delete`, with a "delete although objects or traces are on it" option). The add button
  opens the engine form `level.new`. These three forms live in `crates/engine/src/forms.rs` and
  are also what a typed `level.new`, `level.set`, `level.trace` or `level.delete` starts. To take what
  a dialog sends, `level.trace` accepts a whole-number float for `fade` and a comma-separated text
  (or `*`) for `layers`, besides the JSON forms it took before.
- **Right-hand panels and the layer selector**: the top tool bar starts with the current-layer
  selector (Color and Linetype stay removed). While a sheet-set tab is active its list holds only the
  layers that tab maps (`layer.scope all` lists every layer). The right edge has a rail of four
  vertical titles (Properties, Layers, Blocks, Project, History, Revisions) that is always visible, open or
  collapsed. Clicking a title opens that panel and only that panel (the tab strip inside the panel is
  gone; the panel's own title, such as `Layer . Outline`, sits above it with the Pin and Float
  buttons); clicking the title of the open panel collapses the dock to the rail. Hovering no longer
  opens a collapsed dock; an unpinned open panel still collapses 1.5 s after the pointer leaves.
  Float puts the panels in their own window, and the rail then switches the panel shown there.
  Not tried by hand: Float with the new rail, and the pinned/unpinned auto-collapse.
- **Tab styling** (`apps/aad-winui/SeamlessTab.cs`): the level tabs under the drawing, the drawing tabs
  above it and the panel titles on the right share one look. An inactive tab is flat grey text on
  the bar; the active tab takes the colour of the surface it controls (the drawing, or the open side
  panel), so the two read as one piece with no dividing line, with rounded corners on the outer side and
  a thin accent line on the outer edge. The bars lost their divider lines. Checked by hand in the light
  theme only; the colours come from the theme brushes and the tabs are rebuilt on a theme switch, which
  was not tried.
- **Send selection to a level**: select objects, then either right-click the canvas → *Send to
  Level* → *Move to Level* / *Copy to Level* and pick a level, or right-click a level tab →
  *Send selection here…* / *Copy selection here…*. The shell starts the command form with that
  level preset and prompts on the drawing for an **origin point** on the selection, then a
  **matching point** (Enter = site origin). Geometry is shifted by `to − from` before the move
  or copy so floors drawn offset in one plan can share the site base. Enter on the origin prompt
  keeps world coordinates (previous behaviour). Architecture → Levels → Move / Copy to Level
  opens the same form. In a drawing with one implicit level (a file just opened) the canvas menu
  offers *Move to New Level...* and the Move to Level button asks for a level name: the level is
  created on the way (`create`). Offered only in design view.
- **Level tab state**: a layout (paper space) on screen means no level tab is drawn as the current
  one; clicking a level tab, or `level.activate` / `level.next` / `level.prev`, returns to the Model
  tab showing that level, so the highlighted tab, the displayed drawing and the active level agree.
  Verified by engine tests; the tab bar was not clicked through after this change.
- **Drawing tabs** above the drawing: Design, then one tab per sheet set. A click runs
  `sheetset.activate`. With no sheet sets, a `+ Drawing tabs` tab runs `sheetset.standard` (the
  Project Wizard and `level.setup` also make tabs). Right-click opens Save milestone, Page layout
  and History. The left column is the tool column; its discipline tools follow the active tab.
- **The project's active floor follows the level**: `level.activate` (and so the tabs, Ctrl+PageUp
  and Ctrl+PageDown, `level.next` and `level.prev`) sets the active floor when the level has the
  name of a listed floor. There is no Floor selector (or any context bar) in the WinUI shell
  any more: the level tabs replace it, and `floor.current` still sets the floor for a drawing
  without levels.
- Also fixed on the way: the canvas panicked at start-up on a real GPU ("cad_lines" pipeline) because
  the view uniform was bound to the vertex stage only while the ring shader reads it in the
  fragment stage; both GPU files now bind it to both stages.

Verified: the engine forms and the floor follow are covered by tests (`forms.rs`, `levels.rs`);
the shell itself was built and driven by hand in the running app (tab and rail clicks, the tab
menu, the Level Settings form opened and cancelled, the Trace settings form opened prefilled and
submitted with a changed fade and order, which the engine then reported; a screenshot of each). There is no automated
UI test for the tabs or the rail. Not tried by hand: submitting the Level Settings, New Level and
Delete forms with OK, the add button, the Trace tick, the layers field of the Trace form, and a
drawing with many levels (the tab strip scrolls).

Known limits: the floating Ask pill covers the middle of the bar, so with many levels the later
tabs are expected to sit under it until the strip is scrolled (not tried with more than four); tabs cannot be reordered by
dragging (`level.set order` exists); the rail cannot create or rename sheet sets.

## Risks

- **Drawing-wide code that only reads `model`** (purge, audit, layer rename/delete, find and replace,
  data extraction, `overkill`, xref bind) would delete things used only on another level. Phase 1
  classifies every `.model` user and adds a test per drawing-wide command with content on a second
  level.
- Another CAD program exploding or renaming the `ODRAFT$` blocks loses the level on reload; this is
  reported as a warning.
- Memory: all levels are in memory; entities are shared through `Arc`, but a 20-level tower needs a
  load test.
- Overlap with rendering work in Phase 2.

### Mapping layers on import

- `layer.mapplan {path}` reads a DXF/DWG (changes nothing) and lists its layers with object counts and,
  where the name is recognised by keyword (`AR_WALLS` -> `A-WALL`, `ST_COLUMNS` -> `S-RCC`, `N_TXT` ->
  `AN-TEXT`), the standard layer it most likely stands for, plus the list of standard target layers
  with their tab. Quantity take-off layers (`MEASURE-*`) and `0`/`Defpoints` get no suggestion. Short
  words match whole words only (`MEASURE-DEDUCT` is not a duct).
- `level.import {..., layerMap: {"AR_WALLS": "A-WALL", ...}}` puts the objects of those layers on the
  target layers (built-in layers only, created with the standard look) and does not bring the
  emptied source layers along; the file's layer names are kept in the project entry `layerSources`;
  layers not listed keep their names. Blocks' inner layers are not remapped. The layer mapper for
  opened and imported drawings is in [layer-mapper.md](layer-mapper.md).
- `layer.scope` has a third mode, `level`: the Layers panel lists only the layers used on the active
  level (the current layer stays listed).
- WinUI: Import Level opens a file picker, then a window with one row per layer (suggested target
  preselected). Verified: engine test `a_layer_map_puts_a_files_objects_on_standard_layers_and_the_plan_suggests_them`,
  host test `the_layers_panel_can_list_only_the_layers_used_on_the_active_level`; the shell window and
  the Blocks-panel smart-block list compile but were not clicked through.

## Take-off level ids (engine implemented)

`bridge.export` and `takeoff.record` stamp `level_id` (the level's stable id) on every row whose `level` names one of the drawing's named levels, and add `id` and `elevation_mm` to each entry of the header `levels` list (levels with no project floor of that name are appended). A drawing without named levels writes neither, so existing files and readers are unchanged. The take-off still measures the active level only; record another level with `level`. Optional and additive: no version bump. Contract: MynaOps `docs/bridge/LEVEL-IDS.md`. Tests: `crates/engine/tests/takeoff_record.rs` (`a_drawing_without_levels_writes_no_level_ids`, `named_levels_give_every_row_a_level_id_and_the_header_lists_the_levels`). Not built: recording all levels in one run, WinUI level picker for the take-off.
