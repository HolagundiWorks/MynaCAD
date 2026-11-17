# Levels, sheets and plotting

Detail lives in `docs/levels-and-sheet-sets.md`, `docs/sheet-index.md`, `docs/layer-mapper.md` and `docs/plot-styles.md`. `docs/status.md` is the authority on what is implemented; the notes below say what was run by hand.

## Levels and layer mapping
- **Levels:** named level model spaces in one drawing (`level.*`), traces, move and copy to a level (`level.moveto` with `create: true` also works on a freshly opened file), DXF/DWG round trip.
- **Layer mapper (`layermap`):** puts the layers of a foreign drawing on built-in layers with a suggestion, a preview and an explicit apply; source layer names are kept. The Map Layers wizard opens after a file open or import. Driven by hand: open, review, Back, Cancel and Apply, and Move to Level from the Architecture menu. Driven again (2026-11-15) through the real File > Open: the wizard listed five layers, Review showed 16 objects, Apply left no layer needing mapping. Not driven by hand: the Layer manager entry (its rows are not exposed to UI Automation), "Keep my layers", Import Level.

## Drawing tabs and sheet sets
- Standard tabs: Working drawing, Electrical, Plumbing, Interior, False ceiling (`sheetset.*`).
- **Sections and elevations (Phase 6):** view spaces tied to section and elevation markers, with projection guides drawn faded under the open view (snappable, never saved), `section.*` and `view.*` commands, and a Section / Elevation tab group in the WinUI bottom bar. Layout viewports can show a view (`viewport.set {view}`) and `sheetindex.build` binds view sheets. The tab group was driven by hand through UI Automation: a section made from the New section flyout, switching between a level tab and the view tab.

## Plotting
- **Model-space PDF/SVG/PNG only.** `plot`, `plot.window`, `plotl`, `plotp` and `exportpdf` plot the drawing or a picked window; `plotl` / `plotp` choose landscape or portrait paper and create no layout. A plot style table is chosen with `plotStyleTable`.
- **Removed (package H6, 2026-11-17):** layouts and layout tabs, viewports, page setup, Nameplate designer, sheet templates, the sheet index and issue sets, Master Layouts, Publisher Sets, the Navigator, Batch Plot. There is no layout or sheet printing in MynaCAD until the MynaOps publisher exists; File > Publish starts MynaOps. Foreign DXF/DWG layouts are kept invisibly and written back on export. See `docs/layout-book.md` 11.1.
- Still there: `sheet.scheme` (the numbering scheme set by the Ops-first project configuration), `compose.view.*` (saved display recipes), `printblock*`, revision clouds and the revision register.

## Bridge to MynaOps
Take-off rows carry the level name and, new, `level_id`; the header lists levels with `id` and `elevation_mm`. See `docs/bridge/LEVEL-IDS.md` in MynaOps.

`takeoff.record` with `levels: "all"` (or a list of level names or ids) records several levels in one run, each row with its `level_id`; the header gets an optional `level_status`, and `takeoff.levels` reports which recorded levels changed since (`update` writes the flags into the project file). Run by hand 2026-11-15: a three-level drawing recorded this way was opened in MynaOps (Take-off page level summary and the import level picker, driven with UI Automation). Not run by hand: the stale display, live refresh while recording. The full `cargo xtask ci` and the whole engine test suite were not run (link step out of memory); the take-off and bridge test files, clippy on `aad-engine` and `cargo xtask layers` were.
