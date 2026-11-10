# Sheets and batch plotting

A new drawing has two paper layouts for print: **Nameplate L** (landscape) and **Nameplate P**
(portrait). `sheet.designer` replaces empty Layout1 and Layout2 with those sheets (each with its
own title block and page setup) so each orientation can be designed separately. A drawing that
already has other sheets, or anything drawn on Layout1 or Layout2, is left unchanged. Pass
`landscape: true|false` (default true) and `open: true` to switch to the chosen plate. An older
drawing that still has a single **Title plate** is migrated to Nameplate L once.

Paper layouts are sheets, and they are not shown as tabs. A region of the model is named with `printblock` (a boundary, a name, and a description taken from the level and the tab). Listing a print block stores that description in the drawing so MynaOps can read it. The sheet is composed in MynaOps (Outputs > Layout): a project map, a layout, the print block, the nameplate, and a sheet PDF. That PDF records the sheet. It does not redraw the model. The commands are in `crates/engine/src/cmd/sheet.rs` (and `projfile.rs`
for the project-aware ones); the Plot window only calls them. There is no separate Layout panel: it
was removed, and a saved panel layout that still names it is read as unusable and the default panel
arrangement is used (other preferences are kept). Sheet setup is done with the commands below.

## Live viewports

Panning or zooming inside an MSPACE viewport changes what the viewport shows, so the drawing is
redrawn at once. (Before, that change was applied without telling the canvas to rebuild, so the
viewport could show stale content until another edit.) Regression test:
`crates/engine/tests/viewport_live.rs`.

## Sheet commands

Used from the command line, menus, scripts and the control channel:

| What | Commands |
|---|---|
| Paper size and orientation | `pagesetup` |
| Viewports: scale (typed as `1:100`), lock, remove, add; in a project, which floors and layouts each shows | `viewport.list`, `viewport.set`, `viewport.add`, `viewport.remove`, `floor.viewport` |
| Title block fields, adding a title block | `sheet.fields`, `sheet.titleblock` |
| Ensure Nameplate L and Nameplate P; optionally open one | `sheet.designer` |
| Fit the model into the nameplate viewport (used by Print / plot preview) | `sheet.plotwindow` |
| Saved layouts (apply as the sheet's title card, delete, save) | `sheet.template.*` |

## Title block

`sheet.titleblock` draws a border around the printable area and inserts the block `AAD-TITLEBLOCK`
(180 x 56 mm of real paper size, lower right). Its fields are block attributes: `PROJECT`, `CLIENT`,
`TITLE`, `FLOOR`, `DRAWN`, `DATE`, `SHEET`, `SCALE`, `REV`. They start from the project details,
the floor of the file and the scale of a viewport added afterwards. `sheet.fields` sets them by
tag; any block with attributes of those tags is filled the same way, so a template with its own
title block works too.

## Saved layouts (sheet templates)

A saved layout is everything on the sheet except its viewports (border, title block, notes), the
blocks they use, and the page setup. `sheet.template.save {name, scope}` keeps it in the drawing
(`drawing`, travels with the file) or as a file for the user (`user`, in the MynaCAD settings folder, or
in `folder` for a folder shared by an office). `sheet.template.apply {name, layout?, fields?}` sets
the page up, places the objects and fills the fields. Applying again replaces what an earlier
template put on that layout, never what you drew. A damaged template is refused and changes
nothing. Viewports are not saved: what they show belongs to each drawing.

## Floors in viewports

`floor.viewport {handle, floors?, categories?, plans?}` freezes the Xref layers of the other floors
in one viewport only, so the main drawing carries one sheet per floor, or a floor's plan with its
beam layout over it. The layer's global visibility (`floor.set`) still applies: a layout drawing
must be shown to appear in any viewport.

## Printing and plot preview

Print, Export to PDF and the status-bar **plot preview** first ask for the **plot window** in model
space (two corners, or Extents / Display). The WinUI shell starts `plotl` or `plotp` from the
status-bar orientation (landscape → Nameplate L, portrait → Nameplate P). That region is fitted
into the matching nameplate viewport at the nearest standard scale (`sheet.plotwindow`), the
nameplate is plotted, then the app returns to model space.

- **Print / Export PDF** finishes with a save picker.
- **Plot preview** (PDF icon on the status bar) writes a temporary PDF and opens it in the in-window
  viewer titled "Plot preview".

Edit the plates with **Nameplate** (menu or status-bar button): choose Nameplate L or Nameplate P.
Scripts can pass `window` and `layout` to `sheet.plotwindow`, or call `plotl` / `plotp` / `plot` /
`exportpdf` directly. Batch plotting (`plot.batch`) still plots whole drawings / sheets.

## Batch plot (File > Batch Plot)

`plot.batch {items?, sheets?, includeHidden?, folder?}` writes one PDF per item into the project's
`Exports` folder:

- an item is a floor plan or one of its layouts, read from its **saved** file (a warning says when
  the open drawing has unsaved changes); a file with a sheet (a layout with a viewport) plots the
  sheet, otherwise its model space fitted to the paper;
- by default every item marked for printing, shown and not frozen is plotted; the others are
  reported as skipped with the reason, unless `includeHidden` is set;
- `sheets` are layouts of the drawing you are in (the main drawing); their hidden and frozen Xref
  layers stay out of the output. A drawing outside a project can still plot its own sheets.

The Plot window lists the project's floors with their layouts (ticked when marked for printing),
the sheets of the drawing, a folder, and a Plot button; it shows the files written and what was
skipped.

## Sheet index and numbering

`sheet.index {renumber?, draw?, layout?, at?, height?}` (Architecture > Sheets > Sheet Index):

- Lists every layout except the nameplates with its sheet number, title, scale and revision, read
  from the title block (`SHEET`, `TITLE`, `SCALE`, `REV`). A layout without a `SHEET` value is listed
  under its layout name; one without a `SCALE` uses its first viewport's scale.
- `renumber: {prefix?: "A-", start?: 101, step?: 1}` writes consecutive numbers into the `SHEET`
  field of each layout that has a title block, in layout order. An overflowing range changes nothing.
- `draw: true` draws the list as a table (sheet no., title, scale, revision) on `layout` (default the
  current one) on layer `A-ANNO-SIDX`, top-left at `at` or inside the printable area. Drawing again
  replaces the earlier table on that layout and nothing else.
- Without `renumber` and `draw` it only reads. Tests: `crates/engine/tests/sheet.rs`.

The index is a snapshot: edit a title and run the command again to refresh it. It is not a live field.

## Not implemented

Revision tables, live (self-updating) sheet index, templates that carry viewports, and printing to a
physical device (PDF only).
