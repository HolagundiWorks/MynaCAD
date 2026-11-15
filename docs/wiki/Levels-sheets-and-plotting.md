# Levels, sheets and plotting

Detail lives in `docs/levels-and-sheet-sets.md`, `docs/sheet-index.md`, `docs/layer-mapper.md` and `docs/plot-styles.md`. `docs/status.md` is the authority on what is implemented; the notes below say what was run by hand.

## Levels and layer mapping
- **Levels:** named level model spaces in one drawing (`level.*`), traces, move and copy to a level (`level.moveto` with `create: true` also works on a freshly opened file), DXF/DWG round trip.
- **Layer mapper (`layermap`):** puts the layers of a foreign drawing on built-in layers with a suggestion, a preview and an explicit apply; source layer names are kept. The Map Layers wizard opens after a file open or import. Driven by hand: open, review, Back, Cancel and Apply, and Move to Level from the Architecture menu. Driven again (2026-11-15) through the real File > Open: the wizard listed five layers, Review showed 16 objects, Apply left no layer needing mapping. Not driven by hand: the Layer manager entry (its rows are not exposed to UI Automation), "Keep my layers", Import Level.

## Drawing tabs and sheet sets
- Standard tabs: Working drawing, Electrical, Plumbing, Interior, False ceiling (`sheetset.*`).
- **Sections and elevations (Phase 6):** view spaces tied to section and elevation markers, with projection guides drawn faded under the open view (snappable, never saved), `section.*` and `view.*` commands, and a Section / Elevation tab group in the WinUI bottom bar. Layout viewports can show a view (`viewport.set {view}`) and `sheetindex.build` binds view sheets. The tab group was driven by hand through UI Automation: a section made from the New section flyout, switching between a level tab and the view tab.

## Plotting
- Compose Views, Master Layouts and Publisher Sets with a Navigator, PDF and DWG publish.
- **Sheet index (BIM-style):** sheet records numbered by discipline (A-101, S-201, E-301, P-401 pattern), `sheetindex.build` (a layout per level), a sheet list table, per-sheet revisions, issue sets and publish by issue, discipline or sheet list. The WinUI Sheet index window was opened and driven by hand (UI Automation): the tree listed four sheets (two levels, two sections), Apply fields on a changed number renamed the layout, Renumber and New issue set worked. Real PDF and DWG output through the new publish path has not been produced; drag-to-reorder was not tried.
- The standard title block now carries CHECKED, APPROVED, DISCIPLINE, STATUS and ISSUE; a renumbered sheet renames its layout; `issue.delete` exists. A viewport made at 1:100 on a millimetre drawing shows only part of a tall section: the sheet scale is not chosen from the view's size.

## Planned
A Delete issue button in the Sheet index window, a scale chosen from the view's size for view sheets.

## Bridge to MynaOps
Take-off rows carry the level name and, new, `level_id`; the header lists levels with `id` and `elevation_mm`. See `docs/bridge/LEVEL-IDS.md` in MynaOps.

`takeoff.record` with `levels: "all"` (or a list of level names or ids) records several levels in one run, each row with its `level_id`; the header gets an optional `level_status`, and `takeoff.levels` reports which recorded levels changed since (`update` writes the flags into the project file). Run by hand 2026-11-15: a three-level drawing recorded this way was opened in MynaOps (Take-off page level summary and the import level picker, driven with UI Automation). Not run by hand: the stale display, live refresh while recording. The full `cargo xtask ci` and the whole engine test suite were not run (link step out of memory); the take-off and bridge test files, clippy on `aad-engine` and `cargo xtask layers` were.
