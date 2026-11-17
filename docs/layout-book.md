# Layout book and publisher sets (MynaOps), and the removal of paper space from MynaCAD

Status: **design, written 2026-11-17 for the owner's request of the same day. Nothing in this page is built.** The sections "What exists today" and "Inventory" describe code that was read (each claim names the file). Everything else is planned. Canonical copy: MynaOps `docs/layout-book.md`; MynaCAD carries an identical copy. Written without changing any code. Links to MynaCAD pages (levels-and-sheet-sets, status, sheet-index, plot-styles, NATIVE-OPEN, live-sync and others) resolve in the MynaCAD copy; in MynaOps use the same names under MynaCAD `docs/`.

Owner request (2026-11-17, verbatim, typos cleaned): "paper space is not required, and the layout in MynaCAD: implement how layouts are plotted in ArchiCAD: page set and exporting the drawing to PDF layout sets as per ArchiCAD, and this will live in MynaOps."

Related, read together: [ops-first-workflow.md](ops-first-workflow.md) (ownership of `.osp` keys, the lease, the pipe), [levels-and-sheet-sets.md](levels-and-sheet-sets.md) (levels, sheet sets, Compose Views, Master Layouts, Publisher Sets as built today), [sheet-index.md](sheet-index.md), [plot-styles.md](plot-styles.md), [NATIVE-OPEN.md](NATIVE-OPEN.md), [status.md](status.md).

## 0. Summary

1. MynaOps gets a **Layout Book** (sheets with numbers, subsets, master layouts with title blocks, drawings placed on sheets as live references to saved views) and **Publisher sets** (named lists of sheets with format, pen set, naming, folder). It is written by MynaOps into the shared `.osp` as new `book.*` meta keys.
2. MynaCAD keeps **saved views** (it is the only program that knows the geometry), writes them to the `.osp` as the key `cad.views`, and has a **Publish** shortcut that opens MynaOps. Paper space, layouts, viewports, page setup, the sheet index, compose masters and publisher sets leave the MynaCAD user interface and command set.
3. PDFs are produced by the Rust renderer in a **separate process**: a new engine command `publish.run` (with `publish.check` and `publish.preview`) reached from MynaOps through `aad-cli publish`, JSON in and JSON out. The C# program never draws a model; the AI never computes.
4. Internally the engine keeps the layout and viewport data types: they carry foreign layouts through DXF/DWG round trips, and the publisher builds a **transient, never saved** sheet from the layout book and plots it with the existing, tested renderer.
5. Work packages H1 to H8 (section 11) are ordered so that nothing is removed from MynaCAD before MynaOps can publish.

## 1. What exists today (read)

| Piece | Where read | Fact |
|---|---|---|
| Paper space data | `crates/doc/src/lib.rs` (`Space::{Model, Paper(String)}`), `tables.rs` (`PageSetup`, `Layout {name, tab_order, page, entities, view, master}`), `entity.rs` (`Viewport`: size, view centre/height, lock, frozen layers, layer colours, clip polygon, `level`, `sheet_set`, `traces`, `compose_view`, `view`) | A layout owns an entity store; a viewport shows a level, a sheet set, traces or a section/elevation view space |
| Layout commands | `cmd/layout.rs`, `cmd/layout/*`, `cmd/pagesetup.rs`, `cmd/clipcmd.rs`, `cmd/sheet.rs`, `cmd/view.rs` | `layout`, `layout.new/delete/rename/copy/list/set`, `mview`, `viewport.set/list/add/remove`, `vplayer`, `vpclip`, `mspace`, `pspace`, `page.manager`, `page.toggle`, `pagesetup`, `sheet.page`, `plotstyle`, `stylesmanager`, `plot.window`, `plotl`, `plotp`, `plot`, `exportpdf`, `sheet.titleblock`, `sheet.designer`, `sheet.fields`, `sheet.template.*`, `sheet.index`, `sheet.plotwindow`, `printblock`, `printblock.list/set/delete` |
| Sheet index and issues | `cmd/sheetindex.rs`, [sheet-index.md](sheet-index.md) | Key `sheetidx1` in the drawing's project data: scheme, sheets (number, discipline, title, layout, scale, paper, status, drawn/checked/approved/date, source, revisions), issues (frozen sheet list, `publisherSet`). Commands `sheet.list/new/set/delete/renumber/move/scheme`, `sheetindex.build/fill/table/publish`, `sheet.revision.add/list`, `issue.create/list/delete`. Limits 1000 sheets, 128 issues |
| Compose and publish | `crates/doc/src/compose.rs`, `cmd/compose.rs`, key `compose1` | `ComposeView {id, name, level, sheet_set, scale_denom, plot_style_table, traces, view_center, view_height}` (the ArchiCAD view analogue); `MasterLayout {id, name, page, title_block, template}`; `PublisherSet {name, layouts, format pdf/dwg, single_file, folder}`. `publisher.publish` plots each named layout (`plot(doc, Space::Paper(name), {})`), merges with `pdf_merge` for one file, writes DWG per layout, and by default also runs `takeoff.record` |
| Plot | `crates/io/src/pdf.rs` | `plot(d, space, opts)` returns PDF bytes; `PdfOptions` has paper, landscape, fit, scale, lineweights, plot style table, window (model space only), layers as optional-content groups; `merge(parts)` joins one-page PDFs (cap 256). Paper list in `crates/render/src/paper.rs` (ISO A0 to A5, B4, B5, ANSI A to E, ARCH A to E, custom up to 5080 mm) |
| Pen sets | `crates/doc/src/plotstyle.rs`, [plot-styles.md](plot-styles.md) | Own built-in tables (`default.ctb`, `monochrome.ctb`, `grayscale.ctb`, `default.stb`, `monochrome.stb`, `presentation.stb`); a table has per-style colour, grayscale, screening, linetype, lineweight (hundredths of a mm, 0 to 211); custom tables are kept in the drawing and shadow built-ins of the same name |
| Headless entry | `apps/aad-cli/src/main.rs` | `info`, `convert` (to svg/png/pdf, model space), `run`, `commands`, `mcp`, `perf`, `import`, `auditor`. `info` and `convert` accept `.osp`. No publish verb |
| Foreign layouts | `crates/io/src/dxf_read.rs` (LAYOUT objects, `*PAPER_SPACE` blocks fill `d.layouts`), `dxf_write.rs` (writes layouts and a paper-space viewport), `crates/io/src/native.rs` (stores `layout:<n>` parts) | A DXF/DWG with layouts keeps them as `Layout` records; the `.osp` native model stores them; `flatten.rs` flattens layout stores too |
| MynaOps layout today | MynaOps `BBSApp/Views/LayoutPage.cs`, `Services/SheetLayoutBook.cs`, `Services/PdfExport.cs` (`ExportSheetLayout`), `Office.Shared/DraftPrintBlocks.cs` | Project maps and layouts stored in the project document (`project_doc`); a layout names a print block and nameplate fields; `ExportSheetLayout` writes a QuestPDF page that **records** the sheet and states that it does not redraw the model. Print blocks are read from drawing bytes (`printblocks1` inside `AAD_PROJECT`) |
| MynaOps `.osp` access | `Office.Shared/OdpFile.cs`, `OdpFile.Config.cs` | C# SQLite reader/writer of the same tables as `aad-odp`; `config.*` read/write pattern (package C) |
| WinUI shell (MynaCAD) | `apps/aad-winui/*.cs` | `ComposeNavigator.cs` (Views, Masters, Publisher trees), `PageLayoutWindow.cs`, `PageSetupWindow.cs`, `BatchPlotWindow.cs`, `SheetIndexWindow.cs`, `PlotStyleWindow.cs`, `RevisionsWindow.cs`; `MainWindow.xaml.cs` holds the Orient/Paper boxes (about lines 3475 to 3750) and the plot-on-Nameplate flow (about lines 1470 to 1670) |

Counts read by search (tests included): 216 uses of `.layouts` in 78 Rust files, 139 uses of `Space::Paper` in 56 Rust files. This is why section 8 does not delete the types.

## 2. The ArchiCAD model to follow (own wording, concepts only)

| Concept | Meaning | MynaOps / MynaCAD name here |
|---|---|---|
| **Navigator: Project map** | Tree of the model: stories, sections, elevations, details | MynaOps **Navigator**, left tree "Project": levels (from `config.levels`), sections and elevations (view spaces, read from `cad.views`) |
| **Navigator: View map** | Saved views. A view = what to show (story, section or detail window), **scale**, **layer combination**, **pen set**, model display options, organised in folders | **Saved view** (`cad.views`, created in MynaCAD) shown in the MynaOps **View map** tree, folders kept in `book.viewmap` |
| **Navigator: Layout book** | Ordered sheets in **subsets** (folders). Each **layout** has an **ID** (number), a **name**, a **master layout**, drawings placed on it | **Layout book**: `book.layouts` (subsets) and one `book.layout.<lid>` per sheet |
| **Master layout** | Paper size and the chrome shared by many layouts: border, title block, auto-text fields | **Master layout** (`book.masters`): paper, margins, border, title block elements with field tokens |
| **Drawing** | A placed, live reference to a saved view; shows "changed/unchanged"; has its own number, title and scale label; can be updated | **Placement** inside a layout: `viewRef`, position, size, label; status `up_to_date`, `view_changed`, `model_changed`, `view_missing` |
| **Publisher** | Named **sets** of layouts or views, each with output **format** (PDF, DWG...), options, **destination folder**, **file naming**, one file or one per sheet | **Publisher set** (`book.publishers`): PDF only in v1 (decision D12) |
| **Pen sets** | A named mapping of colours to pen colour and weight, chosen per view or per output | **Pen set** (`book.pensets`) = our plot style table (`PlotStyleTable`, own definitions) |
| **Revisions and issues** | Revision history per layout, an issue register, revision tables on the sheet | **Revisions** and **Issues** (`book.revisions`, `book.issue.<iid>`) carried over from `sheetidx1` / `revisions` |

Clean-room: only these concepts are taken, in our own words and structures. No vendor templates, title blocks, fonts, pen files or help text are copied. Default masters and pen sets are our own definitions and get a row in `ATTRIBUTION.md` only if a file asset is added (data in code needs none).

## 3. Data in the shared `.osp`

### 3.1 Rules

Same rules as [ops-first-workflow.md](ops-first-workflow.md) section 4.1: additive only, new `meta` keys, JSON with `"v"`, strict parse with caps, readers ignore unknown keys and fields, a reader that does not know a version treats that part as absent and says so. No table is added or altered; `meta.format` and `meta.version` stay. A change is one transaction: the changed keys and the bumped `book.rev` raise `meta.revision` once, so live sync carries the book to MynaCAD with no new mechanism. A key is at most 256 KiB (a layout is its own key so the book can grow; see D2).

### 3.2 Keys and owners

| Key | Writer | Content (all `"v":1`) |
|---|---|---|
| `cad.views` | **MynaCAD** (revisioned write on save, like the drawing) | `{views:[SavedView]}`, see 3.3 |
| `book.schema`, `book.rev` | MynaOps | `{v, min_reader}`; `{v, rev, app, time}` |
| `book.viewmap` | MynaOps | `{folders:[{fid, name, parent?}], items:[{vid, folder?, order}]}`: only folders and order of views, never view content |
| `book.masters` | MynaOps | `{next_mid, items:[Master]}`, 3.4 |
| `book.pensets` | MynaOps | `{items:[{name, base?, table}]}`: `table` is the engine's `PlotStyleTable` JSON; `base` names a built-in it extends |
| `book.layouts` | MynaOps | `{next_lid, subsets:[{sid, name, prefix?, parent?, order}], order:[{lid, sid, order}]}`: index only |
| `book.layout.<lid>` | MynaOps | one sheet, 3.5 |
| `book.publishers` | MynaOps | `{next_pid, items:[PublisherSet]}`, 3.6 |
| `book.revisions` | MynaOps | `{next_rid, items:[{rid, code, date, description, by}]}` project-wide revision list |
| `book.issues`, `book.issue.<iid>` | MynaOps | the issue register; one key per issue, 3.7 |
| `book.proposal` | MynaCAD, **local key**, one time | what a legacy drawing already holds (`compose1`, `sheetidx1`, `printblocks1`, layout names), for adoption (section 10) |

MynaCAD **reads** every `book.*` key only through the publish command (section 5); it never writes one. The CLI never writes the project: MynaOps records a run's outcome itself (`book.issue.*` or a short run log). This keeps the single-writer rule of live-sync and [ops-first-workflow.md](ops-first-workflow.md) 2.2: add two rows there (done in this change): `cad.views` to MynaCAD, `book.*` to MynaOps.

Ids (`vid`, `mid`, `lid`, `sid`, `pid`, `rid`, `iid`) are integers, never reused (`next_*`). `number` and `name` are display data and may change; references use ids only.

### 3.3 Saved view (`cad.views`, written by MynaCAD)

Evolves today's `ComposeView` (same type, new fields; the commands `compose.view.*` keep their names or get `view.save`-style aliases decided in H2):

```text
SavedView {
  vid, name (1..64, unique), kind: plan | section | elevation | detail | custom,
  level_cid?            config.levels cid (never the engine LevelId; MynaCAD resolves it with config.levelmap)
  tab_tid?              config.tabs tid: the discipline annotation shown over the level (sheet set)
  view_space?           vid of a section/elevation (a view space), kind section | elevation
  traces[<=8]           level traces as in Viewport.traces (by cid)
  space?                the drawing space (layer combination) name, `space.*`; MynaCAD keeps it in step on rename
  window                {x0,y0,x1,y1} drawing units | "extents"
  scale_denom           1..100000 (1:100 is 100)
  rev                   integer, +1 on every edit of this view by MynaCAD
  level_fp?             MynaCAD's fingerprint of the shown level(s) at save time (the same idea as `takeoff.levels` fingerprints)
}
```

A view holds no pen set (that is chosen by the publisher set or the layout, 7.2) and no geometry. Limit 256 views (the existing `MAX_COMPOSE_VIEWS`). A view whose level, tab or space was deleted is kept with a `broken` note, never dropped silently.

### 3.4 Master layout (`book.masters`)

```text
Master { mid, name, paper ("A1" or custom {w_mm,h_mm}), landscape, margins_mm[4],
         border?{inset_mm, weight_mm},
         elements: [ rect | line | text | field | image | revtable | issuetable | northarrow | scalebar ],
         default_pen_set?, notes? }
element: { kind, x_mm, y_mm, w_mm?, h_mm?, align, font_role, size_mm, weight_mm?, token? | text? | asset? }
```

Coordinates are millimetres from the lower-left of the sheet. Tokens (resolved at publish, unknown token gives an empty field and the warning `unknown-field`): `{project.title}`, `{project.client}`, `{project.address}`, `{project.pid}`, `{team.architect}`, `{company.name}`, `{sheet.number}`, `{sheet.name}`, `{sheet.scale}`, `{sheet.date}`, `{sheet.status}`, `{sheet.drawn}`, `{sheet.checked}`, `{sheet.approved}`, `{sheet.discipline}`, `{rev.code}`, `{rev.date}`, `{issue.name}`, `{page.n}`, `{page.count}`, and `{tb.<key>}` for any key of `config.titleblock.fields`. Text uses the project's font roles (`fonts.resolve`); a missing font falls back deterministically (as [TYPOGRAPHY.md](TYPOGRAPHY.md)); no font file is committed. Defaults: our own masters A0, A1, A2, A3, A4 (a plain border and a simple title block drawn from elements); a logo comes from the company pack asset.

### 3.5 Layout (`book.layout.<lid>`)

```text
Layout { lid, number, name, subset (sid), master (mid), status: draft | review | approved | issued | void,
         discipline?, drawn, checked, approved, date, fields:{...extra tokens...},
         placements: [ { pid, view (vid), x_mm, y_mm, w_mm, h_mm, rotate: 0|90, frame: bool,
                         label: "{view.name}  {scale}", number?, title?,
                         stamp: { view_rev, level_fp, book_rev } } ],
         revisions: [ { rid, note? } ], notes }
```

`number` follows `config.sheets.scheme` (pattern with `{D}` and `{N}`; MynaOps computes the next one per subset or discipline, as `sheet.scheme` does today). A placement's scale is the view's scale (D9). `stamp` is recorded when the user presses Update, so the status `view_changed` (view `rev` differs), `model_changed` (`level_fp` differs) and `view_missing` can be shown, as the Changed/Unchanged state of a placed drawing. Limits: 512 layouts, 32 placements per layout, 64 subsets, strings 200 characters without control characters.

### 3.6 Publisher set (`book.publishers`)

```text
PublisherSet { pid, name, items:[ {layout: lid} | {subset: sid} | {all: true} ],
               format: "pdf", mode: "single" | "per_sheet",
               pen_set (name; default "Default"), colour: "colour" | "grayscale" | "mono",
               paper_override?, folder (absolute or relative to the project),
               naming: pattern with {project} {number} {name} {rev} {date} {issue} {set},
               options: { layers_as_ocg, include_void:false, overwrite: "never"|"replace"|"numbered", compress, outline },
               create_issue?: { name, rev } }
```

Limits: 32 sets, 512 items per set. The existing `PublisherSet` (`format` pdf or dwg, `single_file`, `folder`) maps onto this; `dwg` output is dropped (D12) and `takeoff` on publish is dropped (D8: take-off is MynaCAD's, recorded from MynaCAD).

### 3.7 Issue (`book.issue.<iid>`)

`{iid, name, date, rev (rid), description, set (pid), sheets:[{lid, number, name, rev, status}] (frozen at publish), files:[{path, lid?, pages, bytes, sha256}], source_revision (meta.revision at render), app_version}`. Created by MynaOps after a successful publish when `create_issue` is set. An issue is not editable; `issue.delete` equivalent exists in MynaOps for the register only.

## 4. MynaOps user interface

All pages follow MynaOps' design system; names below are user-visible text.

| Page | Contents |
|---|---|
| **Navigator** (left tree, always in the drawing output area) | Tabs **Project** (levels, sections, elevations), **Views** (view map: folders, drag to reorder, rename folder, shows scale and level; "Edit in MynaCAD" opens MynaCAD on that view through the pipe), **Layouts** (subsets and sheets with number and name), **Publisher** (sets) |
| **Layout book** editor | Left: subset/sheet tree (new subset, new sheet from a master, duplicate, move, renumber by scheme). Centre: the sheet **preview** (rendered by `publish.preview`, section 5; shows the last render and a "render" button; never blocks the page). Right: sheet properties (number, name, master, status, drawn/checked/approved/date, revisions) and placements (add a view, position and size by numbers or by dragging the preview frame, rotate, frame, label, **Update**, status chip). A placement that does not fit shows the number "view window needs W x H mm at 1:S, area is w x h" (the `publish.check` result) |
| **Masters** | List and editor of master layouts: paper, margins, border, elements with a token picker; a render preview with sample data |
| **Pen sets** | List; editor over the plot style table (colour, grayscale, screening, weight 0 to 2.11 mm per colour index), built-ins read only; Duplicate to edit |
| **Publisher** page | Sets list; set editor (items, pen set, colour, mode, naming with a live example, folder picker, options); **Check** (runs `publish.check`, lists warnings), **Preview**, **Publish** (progress per sheet, cancel, the result list with Open file / Open folder, optional issue creation); the issue register and revision table pages |
| MynaCAD | A **Publish** menu item and toolbar button in File and in the Navigator rail: it only activates MynaOps on the Publisher page through the same per-user pipe MynaOps uses for startup (`StartupRequest`, `OpsInstance`) or starts it (`Open MynaOps`). MynaCAD does no publishing |

Not exercised in a window: nothing here exists yet.

## 5. Where PDFs are made: options and recommendation

| | (a) Drive the headless MynaCAD app | (b1) New engine command, separate process `aad-cli publish` | (b2) New `aad-host` C ABI call in MynaOps' process | (c) Draw in C# (QuestPDF) |
|---|---|---|---|---|
| Renderer | the shell's host | Rust (`crates/render`, `crates/io/src/pdf.rs`) | same Rust | a second renderer |
| Crash isolation | good, but a GUI process with a lease | **good: a crash costs one process, MynaOps survives** | **poor: a native fault takes MynaOps down** | good |
| Lease and live state | conflicts with the running managed MynaCAD (one MynaCAD per project) | read-only open of the saved file; no lease | same | none |
| Fidelity with what the user sees | exact | exact (same code as MynaCAD plots) | exact | diverges; clean-room and upkeep cost of a 2D renderer (hatches, linetypes, fonts, dimensions, clipping) in C# |
| Testability | UI automation | **plain Rust tests with fixtures and JSON** | needs FFI tests | C# tests of a new renderer |
| Packaging | GUI exe | one more small exe (`aad-cli`) beside MynaOps | DLL with wgpu dependencies | none |

**Recommendation: (b1).** Reasons: the engine stays Rust and "everything is a command" (`CLAUDE.md`); geometry, clipping and quantities come from deterministic engine code, never from the AI or from a second implementation; the process boundary gives the never-crash property for free (a panic is also caught per sheet by `catch_unwind`, `crates/engine/src/guard.rs`); a read-only open needs no lease; the existing `.osp` open path (`aad-cli info FILE.osp`) is reused. (c) is rejected because the current `ExportSheetLayout` already shows what a C# page can do without the renderer: it records the sheet and cannot draw it. (a) is rejected because the running app holds the project and a GUI is not a batch tool. (b2) is the fallback if process start-up cost proves too high (to be measured in H3; not measured here).

**Unsaved work.** MynaOps first sends `sync.now {flush:true}` to a running MynaCAD (existing pipe request, [ops-first-workflow.md](ops-first-workflow.md) section 15) and waits for its reply; the result records `source_revision` so the issue states which file revision was plotted. If MynaCAD does not answer, MynaOps says so and plots the saved file.

### 5.1 Engine design

- **Parser and validator** `aad_odp::book` (L0, next to `aad_odp::config`): strict parse, caps, finite numbers, ids unique, no `..` in names or folders; shared JSON fixtures read by the C# side (`Office.Shared.LayoutBook`), as package C did for `config`.
- **Compositor** `crates/engine/src/publish.rs` (L4): for each layout, build a **scratch copy** of the drawing, add a transient `Layout` whose page comes from the master and whose entities are the master's elements (lines, text, fields resolved; reuse the title-block drawing code behind `sheet.titleblock`), then one transient `Viewport` per placement bound to the view's level, tab, traces or view space, window and scale (the binding fields `Viewport` already has), then plot `Space::Paper(transient)` with `aad_io::pdf::plot`. The scratch is dropped; the project and the open drawing are never changed. This reuses the renderer and the viewport binding that engine tests already cover (`viewport_levels.rs`, `viewport_views.rs`, `compose_publish.rs`).
- **Pen set**: the set's table is installed in the scratch drawing (`Drawing::plot_style_tables`) and named in `PdfOptions.plot_style_table`; `colour` maps to a built-in table when no pen set is named.
- **Merge** `aad_io::pdf::merge` for `single` mode (cap 256 pages); `per_sheet` writes one file per layout.
- Commands (registered in `crates/engine/src/cmd/publish.rs`, `CommandSpec` with JSON `run`, no prompt form, never a dialog): `publish.check`, `publish.preview`, `publish.run`. Reachable from the control channel, `aad-cli run --cmd`, MCP and scripts as every command is; the agent registry does **not** get them in v1.

## 6. Command and CLI contract

```text
aad-cli publish --request <request.json> [--result <result.json>] [--events]
aad-cli publish --project <X.osp> --set <pid|name> [--out <folder>] [--check | --preview <lid> --dpi 96 --png <file>]
```

Request (JSON, UTF-8, at most 1 MiB; every field optional except `project`):

```json
{ "v": 1, "mode": "run", "project": "C:\\Work\\House.osp",
  "set": 3, "layouts": [12, 14], "folder": "C:\\Work\\Issue\\2026-11-17",
  "overwrite": "numbered", "timeoutSec": 600, "maxPages": 512,
  "preview": { "layout": 12, "dpi": 96, "png": "C:\\Work\\tmp\\p.png" } }
```

`mode` is `check`, `preview` or `run`; `set` and `layouts` select what to plot (`layouts` wins). The whole book is read from the project: `cad.views`, `book.*`, `config.*` and the saved drawing, in one read-only snapshot so that one revision is plotted. `"bookOverride"` (inline book JSON) exists for tests only.

Result (stdout, or `--result` file; one JSON object):

```json
{ "v": 1, "ok": true, "partial": false, "revision": 418, "elapsedMs": 5320,
  "files": [ { "path": "...\\A-101.pdf", "layout": 12, "number": "A-101", "pages": 1, "bytes": 187722, "sha256": "..." } ],
  "warnings": [ { "code": "view-changed", "layout": 12, "placement": 2, "message": "..." } ],
  "errors":   [ { "code": "view-missing", "layout": 14, "message": "..." } ],
  "check":    [ { "layout": 12, "placement": 1, "fits": true, "needMm": [420, 297], "haveMm": [500, 330] } ] }
```

`--events` also writes JSON lines before the result: `{"event":"sheet","index":3,"of":12,"layout":14,"state":"start"|"done"|"failed"}` for progress; MynaOps reads them to drive a progress bar and Cancel (it ends the process).

Exit codes: `0` all done, `1` some sheets failed (the others were written; `partial: true`), `2` the request is invalid, `3` the project cannot be read (not an `.osp`, damaged, a newer `book.schema`), `4` a time or size limit was reached. Messages are English, one line each, with a stable `code` for the shell.

**Never crash** (CLAUDE.md): every number from the book and the request is hostile input (finite, positive, capped: paper 1 to 5080 mm, 100000 for a scale, 512 pages, 32 placements per sheet, 40 MB per sheet PDF as `publisher.publish` already refuses larger); each sheet runs under `catch_unwind`, a failed sheet is reported and the run continues; recursion is bounded by `MAX_BLOCK_DEPTH`; the output name is made safe (`file_stem` rules) and must stay inside the folder (no `..`, no drive change); files are written to a temporary name in the folder and renamed, so a failure leaves no half file; an existing file is replaced only by `overwrite: "replace"` (`never` fails that sheet, `numbered` adds a suffix). The scratch copy of the drawing is made per sheet and released; memory is bounded by the same caps as opening. Regression test names to add: `hostile_book_never_panics`, `hostile_publish_request_never_panics` beside the existing `hostile_params_never_panic`.

## 7. Pen sets, weights, paper, scales, title block, numbering, revisions

### 7.1 Paper and scales

Paper sizes come from the engine's own list (`crates/render/src/paper.rs`: ISO A0 to A5, B4, B5, ANSI A to E, ARCH A to E, custom up to 5080 mm). Scales are the denominators 1, 2, 5, 10, 20, 25, 50, 75, 100, 125, 150, 200, 250, 500, 1000 plus a typed whole number; stored as `scale_denom`. The drawing unit is read from the project (`config.units`; the renderer's `paper_unit_mm` converts). A scale label of a sheet is `1:S` when all placements share one scale, else `As shown`.

### 7.2 Pen sets and weights

A pen set is a `PlotStyleTable` (colour-dependent, 255 colour indices, each with output colour, grayscale, screening, lineweight in hundredths of a mm up to 2.11 mm). Built-ins are the six own tables listed in [plot-styles.md](plot-styles.md); custom ones are in `book.pensets` and are sanitised by the engine on read (`PlotStyleTable::sanitized`). Resolution order for a sheet: layout pen set (if the layout names one) over the publisher set's pen set over `Default` (keeps object properties); `colour: grayscale|mono` maps to the built-in `grayscale.ctb` / `monochrome.ctb` when no pen set is named. Line weights print when the plot's `lineweights` is on (default). A hatch's pattern scale and a linetype's scale come from the drawing and the view's scale; annotation sized for another scale is a MynaCAD matter (`plot.config` / `annotation.layout`), which is why a placement has no scale of its own in v1 (D9).

### 7.3 Title block, numbering, revisions

- The title block is the master's elements; fields come from the tokens in 3.4: project and team from `config.project` and `config.team`, the company from `config.company`, project-wide defaults from `config.titleblock.fields`, per-sheet values from the layout. The existing standard title block with CHECKED/APPROVED/DISCIPLINE/STATUS/ISSUE ([sheet-index.md](sheet-index.md)) is reproduced as the default master.
- Numbering: `config.sheets.scheme` (pattern `{D}-{N}`, width, step, discipline starts) applies to new layouts; renumbering rewrites `number` only, never ids; the file naming pattern uses the number.
- Revisions and issues: `book.revisions` is the project list (code, date, description); each layout lists the revisions that affect it. The master's `revtable` element draws the table (latest first, capped rows) and `issuetable` the issue history. **Revision clouds** are ordinary `revcloud` entities in model space (`revision.clouds` exists today, [revision-history.md](revision-history.md)) and print with the view; v1 does not tie a cloud to a revision, so a cloud shows while its layer is visible in the view's space (D10 proposes a layer-per-revision convention). Today's `revision.issue` (title-block `REV`, tables on `A-ANNO-REVT`) is superseded for sheets; the project history and milestones in MynaCAD (`history.*`) stay.

## 8. Removing paper space from MynaCAD: inventory and decisions

"Removed" here means: no layout tab or command or dialog for the user, no layout-based plotting. The data types stay internal (decision D4).

### 8.1 Inventory (read) and decision

| Area | Items | Decision |
|---|---|---|
| `crates/doc` | `Space::Paper`, `Layout`, `PageSetup`, `Viewport`, `Drawing.layouts`, `Drawing::layout`; `compose.rs` (`ComposeView`, `MasterLayout`, `PublisherSet`, `ComposeBundle`); `sheetset.rs`/`levels.rs` viewport bindings | **Keep** `Space`, `Layout`, `PageSetup`, `Viewport` as internal data: DXF/DWG round trip, native `.osp` parts `layout:<n>`, and the transient publish sheet. **Evolve** `ComposeView` into `SavedView`. **Delete** `MasterLayout` and `PublisherSet` from the doc after migration (H6): they move to the book |
| Commands deleted | `layout`, `layout.new/delete/rename/copy/list/set`, `mview`, `viewport.set/list/add/remove`, `vplayer`, `vpclip`, `mspace`, `pspace`, `page.manager`, `page.toggle`, `pagesetup`, `sheet.page`, `sheet.titleblock`, `sheet.designer`, `sheet.fields`, `sheet.template.*`, `sheet.index`, `sheet.plotwindow`, `sheet.list/new/set/delete/renumber/move/scheme`, `sheetindex.*`, `sheet.revision.*`, `issue.*`, `master.*`, `publisher.*`, `plot.batch`, `floor.viewport`, `printblock*` (after conversion to saved views, H2) | **Delete** in H6 after H5 ships. The title-block drawing code and viewport tessellation are reused internally by `publish.*` |
| Commands kept | `plotl`, `plotp`, `plot.window`, `plot`, `exportpdf` in **model space with a window** (a quick PDF of the drawing, ops-first 3.4 keeps PDF export; D7); `plotstyle`, `stylesmanager` (object plot styles, table editing moves to MynaOps pen sets but the engine tables stay); `plot.config`, `plot.validate`, `annotation.layout` (annotation sizing for a scale); `compose.view.*` (as saved views); `section.*`, `view.*` view-space commands; `revision.clouds`, `revcloud`; `history.*`; `levels`, `sheetset.*` | **Keep** |
| `crates/io` | `dxf_read.rs`, `dxf_write.rs`, `native.rs`, `levels_file.rs` (`VPBIND`, `compose1`), `pdf.rs` | **Keep** all: foreign DXF/DWG layouts are still read and written; `pdf.rs` is the publisher's output. **Delete** only `compose1` publisher/master parts after migration |
| DXF/DWG import into the project | `level.import` reads model space only | A foreign drawing's layouts are **not imported**. The import report gets a list: layout names, entity and viewport counts, "not imported: sheets are made in MynaOps" (H7). Opening/importing must not crash on any layout data (`hostile_dxf_does_not_panic` stays and gains a layout case). Optional later: import a layout's paper annotation as sheet-set annotation (D5) |
| `.osp` | native model parts `layout:<n>`, project keys `compose1`, `sheetidx1`, `printblocks1`, `revisions` | **Keep the bytes** (no data loss, DXF export still writes layouts); MynaCAD stops showing or editing them; `compose1`, `sheetidx1`, `printblocks1` are read once for adoption (section 10) and then ignored |
| Native working copy and managed import | `NATIVE-OPEN.md`, standalone open | Keep; standalone MynaCAD opens a foreign file with its layouts intact and invisible, and DXF/DWG export writes them back |
| `crates/aad-host`, `chrome.rs`, `overlay.rs`, `lib.rs` | layout state, `Model` and layout tabs in `chrome_json`, paper overlay | **Delete** layout tab state and paper overlay in H6; `chrome_json` loses `layouts` |
| `apps/aad-winui` | `ComposeNavigator`, `PageLayoutWindow`, `PageSetupWindow`, `BatchPlotWindow`, `SheetIndexWindow`, status-bar layout tabs (`ViewTabs.cs`, not read in detail), the plot-on-Nameplate flow and paper boxes in `MainWindow.xaml.cs`, File > Page Setup Manager, Batch Plot, Publish | **Delete** in H6; File > Publish becomes the shortcut to MynaOps; `PlotStyleWindow` may stay for object styles |
| `crates/ui-egui`, `agent`, `mcp`, `auditor` | layout and plot tools, command parity checks | Remove layout entries with the commands; `cargo xtask parity` regenerates `parity-checklist.md` |
| Tests | 78 files touch layouts; named suites: `layout_prompts.rs`, `annotation_layout.rs`, `sheet.rs`, `compose_publish.rs`, `viewport_levels.rs`, `viewport_views.rs`, `sheet_index*`, `exchange.rs` | Tests of deleted commands are deleted with them; tests of the renderer through a viewport become tests of `publish.run` |
| Docs | [levels-and-sheet-sets.md](levels-and-sheet-sets.md) (Compose and publish, Phase 4), [sheet-index.md](sheet-index.md), [sheets.md](sheets.md), [plot-styles.md](plot-styles.md), [status.md](status.md) rows "Layouts and output", "Structured sheet set", "Compose and publish", "Layout viewports by level", "Sheets and printing", "Drawing spaces..." | Marked superseded in this change (pointers); rewritten with H6 |

### 8.2 Why the types stay

216 uses in 78 files, plus the DXF/DWG layout round trip (`dxf_read.rs`, `dxf_write.rs`), the `.osp` native parts and the transient publish sheet, all depend on them. Deleting them would lose layouts on export of a foreign drawing and force a renderer rewrite. The user-facing removal is complete without that; an internal-only cleanup (H8) can narrow `Space::Paper` use later.

## 9. What MynaCAD keeps

Saved views (create, edit window and scale, rename, delete, list, "Edit in MynaCAD" target from MynaOps); levels, tabs, view spaces and drawing spaces (layer combinations) that views refer to; **Send to MynaOps** (take-off record, save, sync) as today; model-space quick PDF/SVG/PNG export of a window; export of DXF/DWG; revision clouds and history; the **Publish** shortcut that opens MynaOps' Publisher page. A saved view is created from the current display (level, tab, layer space, window, scale) with one command, `compose.view.save` today, and listed in a Views panel that replaces the Navigator's Views tree.

## 10. Migration and adoption

- **Existing drawings** with `compose1`, `sheetidx1`, `printblocks1` or layouts: on managed open MynaCAD writes the local key `book.proposal` (like `config.proposal`, [ops-first-workflow.md](ops-first-workflow.md) 4.5): compose views (become saved views, written to `cad.views` at once), masters (name, paper, title block name), publisher sets, sheet records (number, title, scale, paper, status, revisions), issues, print blocks (each becomes a saved view with that boundary as window), layout names.
- **MynaOps** offers **Adopt layouts into the layout book** (preview, then write `book.*` in one revision; nothing is deleted from the drawing). The MynaOps project maps and layouts stored in `project_doc` (`SheetLayoutBook`) are read once and converted (a map becomes a subset, a layout a sheet, its print block a placement of the converted view); the old `LayoutPage` is removed after H5.
- A layout whose viewport shows a level and a sheet set becomes a saved view plus a placement; a viewport with a clip polygon or a frozen-layer list that a saved view cannot express is reported as `not converted` with its name (v1 saved views carry a layer space, not per-viewport freezes).
- Old files: a project with no `book.schema` behaves as before the book exists. A reader that does not know `cad.views` or `book.*` ignores them.

## 11. Work packages (one session each, in this order)

Rule for every package: document first (update this page and the affected pages), implement, run `cargo xtask ci` (MynaCAD) or the MynaOps test projects, update the docs with what was built and run, clean up own temp files and builds. Each is on its own branch in both repos where noted.

| Pkg | Scope | Main files | Acceptance |
|---|---|---|---|
| **H1 Contract** | `aad_odp::book` (types, strict parse, validation, caps), shared JSON fixtures, MynaOps `Office.Shared.LayoutBook` read and write, ownership rows, `book.schema`/`book.rev`; no UI | `crates/odp/src/book*.rs`, MynaOps `Office.Shared/LayoutBook.cs`, fixtures, `docs/PROJECT-FILE.md` | Both sides parse every fixture to equal values; a hostile set (huge, NaN, duplicate ids, `..` in names, unknown version) is refused or ignored, never a panic or exception; one write raises `meta.revision` once |
| **H2 Saved views** | Evolve `ComposeView` into `SavedView` (kind, `level_cid`, `tab_tid`, window, space, `rev`, `level_fp`), write `cad.views` on save, convert print blocks to views, Views panel replaces the Navigator Views tree, `broken` notes | `crates/doc/src/compose.rs`, `cmd/compose.rs`, `cmd/printblock.rs`, `crates/io` (meta write), `apps/aad-winui/ComposeNavigator.cs` | Save two views, reopen, `cad.views` holds both with cids; a rename of a level keeps the link; a deleted level marks the view broken; old `compose1` still loads; tests `saved_views` |
| **H3 Publisher engine** (may split a/b) | `publish.check/preview/run`, compositor, master elements and tokens, pen set install, naming, merge, atomic writes, limits, exit codes, `aad-cli publish`, `--events`, `bookOverride` | `crates/engine/src/publish.rs`, `cmd/publish.rs`, `apps/aad-cli/src/main.rs` | On a fixture `.osp` with two levels, one master, three sheets: `run` writes PDFs whose page sizes, placement rectangles and title-block text match expectations (checked from the PDF content); `check` reports a non-fitting placement; a missing view yields `view-missing` and the other sheets still write; `hostile_*` tests; timing of start-up and of a 3-sheet run recorded (to decide b2) |
| **H4 MynaOps Navigator and Layout book** | Navigator tabs, View map (folders), Layout book editor, masters editor, preview through `publish.preview`, numbering by scheme, placement status | MynaOps `BBSApp/Views/*`, `Services/LayoutBook*.cs`, installer ships `aad-cli` (D14) | Create subset, sheet, place a view, move and update; status chips change when MynaCAD edits the view (live sync); the page never blocks while a preview renders; UI Automation run by hand on the real apps, recorded |
| **H5 Publisher page, issues, revisions** | Publisher sets editor, check, run with progress and Cancel, results list, issue creation, revisions, pen set editor | MynaOps `Views/Publisher*.cs`, `Office.Shared/LayoutBook.cs`, pipe `sync.now {flush:true}` before a run | Publish a 3-sheet set as one PDF and as per-sheet files; an issue is recorded with sha256 of each file; cancelling leaves no half file; a failed sheet is reported and the rest are written |
| **H6 Remove paper space UI and commands** (two sessions: a commands/engine, b shell/docs) | Delete the commands and dialogs of section 8.1, the **Publish** shortcut, `chrome_json` layouts, tests of removed commands; keep internal data | `crates/engine/src/cmd/*`, `crates/aad-host`, `apps/aad-winui`, `docs/status.md`, parity checklist | `cargo xtask ci` green; the command catalog has none of the deleted ids; `hostile_dxf_does_not_panic` with layouts passes; opening and DXF-exporting a foreign drawing with layouts round-trips them; Publish opens MynaOps on the Publisher page (hand run) |
| **H7 Adoption and import report** | `book.proposal`, MynaOps Adopt layouts, import report lists foreign layouts | `crates/engine/src/cmd/import.rs`, MynaOps adoption | A legacy project's compose views, sheets and issues appear in the book after adopting; nothing deleted from the drawing; the report names each skipped foreign layout |
| **H8 End to end and cleanup** | Both real apps: create views, place, publish, issue; remove dead code left by H6 (`MasterLayout`, `PublisherSet`, `compose1` writers), narrow `Space::Paper` where safe | both repos | Written run log in [ops-first-workflow.md](ops-first-workflow.md) style; all docs reconciled; leftover list |

Order: H1, then H2 and H3 in parallel (disjoint files), H4, H5, H7, **then** H6 (users keep layouts and Publish until MynaOps replaces them), H8. H4 needs H3's `publish.preview`.

## 12. Decisions the owner must make (recommendation first)

| # | Decision | Recommendation |
|---|---|---|
| D1 | How MynaOps gets PDFs | (b1) separate process `aad-cli publish`; (b2) in-process DLL only if start-up is too slow |
| D2 | Where the book lives in the `.osp` | `book.*` meta keys, one per layout; new tables only if 512 sheets proves too large |
| D3 | Who creates saved views | MynaCAD creates and edits (it holds the geometry); MynaOps lists, organises in folders and places them |
| D4 | Depth of paper-space removal | Remove UI and commands; keep `Layout`/`Viewport`/`Space::Paper` internal for DXF/DWG round trip and the transient sheet |
| D5 | Foreign DXF/DWG layouts on import | Not imported; listed in the import report. Optional later: layout annotation as sheet-set annotation |
| D6 | Existing compose, sheet index, publisher in MynaCAD | Migrate once through `book.proposal`, then delete (no parallel run) |
| D7 | Quick model-space PDF export in MynaCAD | Keep (ops-first 3.4) |
| D8 | Take-off on publish | Drop; take-off stays a MynaCAD action |
| D9 | Scale of a placed drawing | Fixed to the view's scale in v1; no per-placement override |
| D10 | Revision clouds and revisions | v1: clouds print with their layers; later a layer-per-revision convention (`A-REV-<code>`) |
| D11 | PDF outline (bookmarks per sheet) and one multi-page file | Both modes in v1; outline in H5 if cheap |
| D12 | Publisher output formats | PDF only in v1; DWG/DXF stay MynaCAD exports (there are no layouts to publish) |
| D13 | Default masters | Our own A0 to A4 set drawn from elements, logo from the company pack |
| D14 | `aad-cli.exe` shipped with the suite installer, found beside MynaOps (`OSTUDIO_CAD_CLI` override) | Yes |
| D15 | When to delete paper-space UI | After H5 ships and an end-to-end hand run, not before |

## 13. Out of scope and risks

Out of scope: 3D, a constraint solver, DWG/DXF publishing, a PDF/A or signed PDF claim, scale-dependent annotation per placement, the AI choosing views or numbers, MynaHub and ArchiMan. Risks: the renderer's page-size mix in one merged PDF was not verified (`merge` takes one-page parts; H3 tests it); `aad-cli` open of an `.osp` while MynaCAD holds the lease was not tested here (it is a read-only SQLite snapshot in WAL mode by the live-sync design, [live-sync.md](live-sync.md), to be proven in H3); start-up time of `aad-cli` on a large project is unmeasured; text fonts in a headless run depend on installed fonts (fallback is deterministic); placement/preview UI in MynaOps needs a drag surface that WinUI supports but was not prototyped.

## 14. What was read and what was not run

Read: both repos' `CLAUDE.md`, [ops-first-workflow.md](ops-first-workflow.md) (sections 2 to 4 in detail, the rest by headings), [levels-and-sheet-sets.md](levels-and-sheet-sets.md) in full, [status.md](status.md), [sheet-index.md](sheet-index.md) (head), [plot-styles.md](plot-styles.md) (head), `crates/doc` layout, viewport, compose and plot style types, `cmd/compose.rs` publish code, the command registrations of the layout and sheet files, `crates/io/src/pdf.rs` (options, plot, page setup), `dxf_read.rs`/`dxf_write.rs`/`native.rs` layout hits, `flatten.rs`, `apps/aad-cli/src/main.rs` usage, MynaOps `LayoutPage.cs`, `SheetLayoutBook.cs`, `ExportSheetLayout`, `DraftPrintBlocks.cs`, `OdpFile.cs` schema. Not read in detail: [NATIVE-OPEN.md](NATIVE-OPEN.md) and [ARCHITECTURE.md](ARCHITECTURE.md) beyond what [status.md](status.md) quotes, `ViewTabs.cs`, `MainWindow.xaml.cs` outside the paper lines, the egui crate, MynaOps' installer. No code was changed, no build and no test was run for this page.
