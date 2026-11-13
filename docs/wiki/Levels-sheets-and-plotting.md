# Levels, sheets and plotting

Detail lives in `docs/levels-and-sheet-sets.md`, `docs/sheet-index.md`, `docs/layer-mapper.md` and `docs/plot-styles.md`. `docs/status.md` is the authority on what is implemented; the notes below say what was run by hand.

## Levels and layer mapping
- **Levels:** named level model spaces in one drawing (`level.*`), traces, move and copy to a level (`level.moveto` with `create: true` also works on a freshly opened file), DXF/DWG round trip.
- **Layer mapper (`layermap`):** puts the layers of a foreign drawing on built-in layers with a suggestion, a preview and an explicit apply; source layer names are kept. The Map Layers wizard opens after a file open or import. Driven by hand: open, review, Back, Cancel and Apply, and Move to Level from the Architecture menu. Not driven by hand: the Layer manager entry, "Keep my layers", Import Level and the level tabs.

## Drawing tabs and sheet sets
- Standard tabs: Working drawing, Electrical, Plumbing, Interior, False ceiling (`sheetset.*`).
- **Sections and elevations (Phase 6):** view spaces tied to section and elevation markers, with projection guides drawn faded under the open view (snappable, never saved), `section.*` and `view.*` commands, and a Section / Elevation tab group in the WinUI bottom bar. Built and tested in the engine and host; the tab group was started but not clicked through by hand.

## Plotting
- Compose Views, Master Layouts and Publisher Sets with a Navigator, PDF and DWG publish.
- **Sheet index (BIM-style):** sheet records numbered by discipline (A-101, S-201, E-301, P-401 pattern), `sheetindex.build` (a layout per level), a sheet list table, per-sheet revisions, issue sets and publish by issue, discipline or sheet list. The WinUI Sheet index window compiles but was not opened by hand; real PDF and DWG output through the new publish path has not been produced.
- A sheet made from a section or elevation view gets a layout without that view's content: a viewport cannot be bound to a view space yet.

## Planned
Extra title-block fields (CHECKED, APPROVED, DISCIPLINE, STATUS, ISSUE), binding viewports to views, deleting an issue set, renaming a layout when its sheet is renumbered.

## Bridge to MynaOps
Take-off rows carry the level name and, new, `level_id`; the header lists levels with `id` and `elevation_mm`. See `docs/bridge/LEVEL-IDS.md` in MynaOps.
