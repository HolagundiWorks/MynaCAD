# Structured sheet set: sheet index, revisions and issue sets

Status: engine commands and the WinUI **Sheet index** window are built; see "Verification" at the end for what was run. Planned items are marked as such. Related: the Compose and
publish section of [levels-and-sheet-sets.md](levels-and-sheet-sets.md) (Compose Views, Master
Layouts, Publisher Sets), [sheets.md](sheets.md) (title block, `sheet.index`),
[revision-history.md](revision-history.md) (project revision register), [plot-styles.md](plot-styles.md).

## Why

Plotting needs what an ArchiCAD or Revit sheet index gives: a numbered, ordered list of sheets
grouped by discipline, each with title-block data, a revision history, and named issues (a set of
sheets frozen at a date and revision) that can be published. Today MynaCAD has layouts, masters and
publisher sets but no record per sheet and no issue concept. `sheet.index` is a snapshot read from
title blocks; it does not own anything.

## Data model

One JSON document in the drawing's project data under the **new key `sheetidx1`** (`Drawing::project`,
the same map that already carries `compose1`, `revisions`, `details`). Nothing else changes in the
file: `.osp` and DXF/DWG already round-trip the project map, MynaOps shares the `.osp` and ignores
unknown keys, and a drawing without the key behaves as before. Damaged JSON is treated as "no sheet
index" and never panics.

```text
SheetIndex {
  v: 1, seed, issueSeed,
  scheme:      { pattern, width, step, disciplines: [{code, name, start}] },
  sheets:      [Sheet]            // order = index order inside each discipline; disciplines keep scheme order
  issues:      [Issue],
  tables:      { "kind\layout": [handle hex] }   // objects a drawn table owns, so a redraw replaces them
}
Sheet {
  id (never reused), number, discipline, title, layout (layout name; "" until built),
  scale ("1:100"), paper ("A1"), landscape,
  status: draft | review | approved | issued | void,
  drawn, checked, approved, date,
  source: { kind: custom | level | composeView | view, level?, composeView?, view? },
  revisions: [{ rev, date, description, by, issue? }]
}
Issue { id, name, date, rev, description, frozen: [{sheet id, number, title, layout, rev, status}], publisherSet }
```

Limits: 1000 sheets, 64 revisions per sheet, 128 issues, 64 disciplines, strings 200 characters
without control characters, number unique case-insensitively.

### Numbering scheme

`pattern` is a template with `{D}` (discipline code) and `{N}` (sequence, zero-padded to `width`);
default `{D}-{N}`, width 3, step 1. Each discipline has a `start`. Defaults, our own and editable:

| Code | Discipline | Start |
|---|---|---|
| G | General | 001 |
| A | Architectural | 101 |
| S | Structural | 201 |
| E | Electrical | 301 |
| P | Plumbing | 401 |
| M | Mechanical (HVAC) | 501 |
| I | Interior | 601 |

So `A-101`, `S-201`, `E-301`, `P-401`. A pattern must contain `{N}` exactly once and at most one
`{D}`; characters are limited to letters, digits and `- _ . space {}`. **Renumber** assigns
`start + position * step` per discipline in current order and is all-or-nothing: collisions with a
manual number outside the sequence are reported and nothing changes.

### Revision

A revision belongs to a sheet (`rev` code, date, description, by, optional issue name). The sheet's
**current revision** is the last entry and fills the `REV` title-block field. The
**revision schedule** is the flat list over all sheets, newest first. The existing project-wide
register (`revision.issue`, key `revisions`) is unchanged and stays the place for clouds and the
per-drawing revision table; sheet revisions are separate commands (`sheet.revision.*`) because
`revision.list` is already taken.

### Issue set

`issue.create` names an issue, takes a date and a revision code, picks sheets (all non-void, one
discipline, or an explicit list), and **freezes** them: it records number, title, layout, current
revision and status per sheet, stamps each with a sheet revision for that issue (unless `stamp:false`),
and sets their status to `issued`. The frozen list never changes afterwards, so a later renumber or
title edit does not rewrite history. It also creates a Publisher Set named `Issue <name>` (only for sheets that have a layout) with those
layouts in index order. `issue.list` shows them. Deleting a sheet that is in an issue is allowed; the
frozen row keeps its number and shows `layout missing` when published.

## Index generation

`sheetindex.build` creates sheet records and layouts from what the drawing has:

1. one sheet per **level** (`A-101 Ground Floor Plan`, scale 1:100 by default) with one viewport
   bound with `viewport.set {level}`; a level that already has a sheet is skipped (build is repeatable);
2. one sheet per **Compose View** when asked (`composeViews: true`), applied with `compose.view.apply`;
3. **section / elevation views**: sheets can name a `view` source. The module asks one function,
   `view_sources(session)`, for the section and elevation view spaces the drawing has (Phase 6, see
   `levels-and-sheet-sets.md`). A sheet whose view cannot be resolved is still created, without a
   viewport, with a warning. Limit: `viewport.set` binds a level, not a view space, so a sheet made
   from an existing view gets its layout and numbering but no view content yet.

Each layout is named `<number> <title>` (limits of `layout.new` apply), takes the master layout when
`master` is given (`master.apply`: paper and title block) or the page `paper`, and gets the title
block filled. All of it goes through the existing commands; nothing here plots or computes geometry.

`sheetindex.table` draws the index as a table on a layout (default `G-001 Sheet Index`, created
when missing): columns number, title, scale, paper, revision, status; one band per discipline; a
second kind (`kind: revisions`) draws the revision schedule. Our own lines and text on layer
`A-ANNO-SIDX`; a redraw replaces what the previous draw put there (`tables`). It is a snapshot, like
`sheet.index`; redraw after changes.

## Title-block fields

`sheet.set` and `sheetindex.fill` write these attribute tags into the sheet's title block with the
existing `write_fields` path, so any block with those tags works and tags a block lacks are skipped:
`SHEET` number, `TITLE`, `SCALE`, `REV`, `DRAWN`, `CHECKED`, `APPROVED`, `DATE`, `DISCIPLINE`,
`STATUS`, `FLOOR` (source level), `PAPER`, `ISSUE` (latest issue). `PROJECT`, `CLIENT` come from
project details through `sheet.titleblock` as before. The standard `AAD-TITLEBLOCK` has the nine
original tags only; a title block with the extra tags is a later, separate piece (planned).

The layout name is fixed when the layout is made (`<number> <title>`); a later renumber or title
change updates the title block and the record, not the layout name, because publisher sets and
issues refer to layouts by name.

## Publishing

`sheetindex.publish {issue | discipline | sheets, format?, singleFile?, folder?, takeoff?}` resolves
the layouts (void sheets and sheets without a layout are skipped with a warning), upserts a
Publisher Set (`Issue <name>` for an issue, `Discipline <code>` otherwise) and calls
`publisher.publish` on it. PDF one-file-per-sheet or multipage, DWG, fit checks and the take-off
record are exactly those of the Publisher; plot styles come from each layout's plot style table.

## Commands

| Command | Purpose |
|---|---|
| `sheet.new / set / delete / list` | Sheet records. `set` accepts every field and refills the title block |
| `sheet.renumber` | Per discipline, or all; `dryRun` previews |
| `sheet.move` | Move a sheet before/after another or to a position; `discipline` change renumbers it |
| `sheet.scheme` | Read or set pattern, width, step, discipline list |
| `sheetindex.build` | Sheets and layouts from levels, compose views, views |
| `sheetindex.fill` | Rewrite title-block fields from records |
| `sheetindex.table` | Draw the sheet list or revision schedule |
| `sheet.revision.add / list` | Per-sheet revision history; `list` without a sheet is the schedule |
| `issue.create / list` | Frozen issue sets |
| `sheetindex.publish` | Publish by issue or discipline through the Publisher |

All are ordinary commands (JSON form with defaults, no dialogs). Undo covers them because they only
change the drawing's project data and layouts.

## Interface (WinUI)

`apps/aad-winui/SheetIndexWindow.cs` (menu Architecture > Sheets > Sheet Index, or the commands `sheet.list`, `sheet.new`, `sheetindex.build`, `sheetindex.publish`, `issue.create`) is a window with a **Sheet Index** tree (disciplines, sheets as `A-101  Ground Floor Plan  1:100  Rev B`),
buttons Add / Renumber / Move up / Move down / Edit fields / Build from levels / Draw table, an
Issue list with New Issue, and a Publish dialog choosing an issue, a discipline or the whole index,
PDF or DWG, one file or many. It calls the commands above only.

## Not in scope

No proprietary title blocks or numbering standards are copied; the scheme is ours and editable. No
automatic revision clouds (see `revision.clouds`). No live fields in the drawn table.

## Verification

Run: `cargo test -p aad-engine --test sheet_index --test sheet_index_publish` (14 + 2 tests: numbering,
renumber/move, build from levels and compose views and a master, missing views, title-block fill,
revisions, issues frozen across renumbering, drawn table and redraw, key round trip and undo, damaged
stored data, about 50 hostile parameter cases, the 1000-sheet cap, publish by issue and by discipline
with fake plot and DWG hooks); `dotnet build` of `Aad.WinUi.csproj` (0 warnings).
Not run: the WinUI window was compiled but not opened, so tree drag-to-reorder, the buttons and the
publish dialog were not clicked; real PDF and DWG output through this path was not produced (the tests
use fake writers); `sheetindex.build` was not tried on a large drawing.
Planned: a title block with CHECKED, APPROVED, DISCIPLINE, STATUS and ISSUE attributes; section and
elevation sources once view spaces exist (`view_sources` in `sheetindex.rs` is the single hook);
renaming a layout when its sheet is renumbered; deleting an issue set.
