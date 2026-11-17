# Projects, floors and external references

A project is a folder of drawings joined by external references (Xrefs). Every drawing is an
ordinary DXF that opens anywhere; paths are relative, so the folder can be moved or backed up as a
whole. The commands are in `crates/engine/src/cmd/xref.rs` and `projfile.rs`; the tests run on real
files (`apps/aad-cli/tests/xref.rs`, `project_floors.rs`).

## Xrefs

| Command | What it does |
|---|---|
| `xref.attach {path, name?, at?, scale?, rotation?, overlay?}` | Reads the drawing (flattened to 2D), copies its model space into a block, and inserts it. Its layers come in as `<name>\|<layer>`, nested blocks as `<name>\|<block>` |
| `xref.reload {name?}` | Reads the source again; layer flags set here are kept. A missing source is reported and the last content stays |
| `xref.detach {name}` | Removes the references, the block and its layers |
| `xref.bind {name}` | Turns it into an ordinary block (`<name>$0$<layer>` layers) and cuts the link |
| `xref.list` | Name, stored path, whether it resolves, object count, references |

Paths are stored relative to the saved drawing. A source in other units is inserted scaled by the
ratio of the units, never by rescaling its geometry (if either drawing has no units set, there is a
warning and no scaling). An **overlay** (`overlay: true`) exists only for the session: it is never
written when the drawing is saved. Not implemented: clipping, fading, and per-reference layer
overrides beyond the layer table.

## Project layout

```text
<Project>/
  project.aad.json                    the manifest (the one record of the project)
  <Name>_Main.dxf                     every floor and layout attached at the same origin
  Floors/<Floor>.dxf                  the architectural plan of one floor
  Layouts/<Floor>/<Layout>.dxf        beam, ceiling, furniture, electrical, plumbing, custom
  Exports/
```

`project.setup {name, folder, floors, categories, units, scale, client, location, address, type}`
creates the folders and files, writes the manifest, builds the main drawing and opens it.
`floors` is a list (`{name, height, elevation?, kind?}`, bottom to top) or `{count, basements?,
terrace?}`; the ground floor sits at 0 and the others are stacked from it. Each file gets the
project details, the floors (heights in metres), the layers of its discipline and a pointer to the
manifest. Layouts start hidden. `project.open {path}` opens the main drawing from the project
folder, `project.aad.json` or any file in the project, re-reads the references and re-applies the
saved visibility.

## Floors

- `floor.activate {name, category?}` opens that floor's file (or its layout's) for editing and shows
  the other visible floors and layouts in it as **locked overlays**. Everything drawn goes into
  that file. Only the active floor and its layouts are unlocked in the manifest.
- `floor.set {name, category?, visible?, frozen?, locked?, print?}` changes one floor or layout.
  The flags are saved in the manifest and applied at once to every open drawing of the project:
  the layers of the xref in the main drawing and of the overlay in the drawing being edited.
- `floor.list` reports the floors, their layouts and flags.

Floors and layouts are independent. For example, the first floor's plan can be edited with the
ground floor's beam layout shown over it while the ground floor plan itself is hidden.

## Status against the specification

| Item | State |
|---|---|
| Xref attach, reload, detach, bind, overlays, relative paths | Implemented and tested |
| Project folder, floor files, layout files, main drawing, manifest | Implemented and tested |
| Floor switching; new work goes into the active floor's file | Implemented and tested (`floor.activate`) |
| Show, freeze, lock, print flags per floor and per layout; overlays | Implemented and tested |
| Moving and reopening a project keeps references and flags | Implemented and tested |
| Floor selector in the top toolbar, active floor shown in the interface | Not built |
| Project Setup Wizard (dialog, steps, review) | Not built; the typed manifest, planner and company profiles under it are built (`project.plan`, [PROJECT_WIZARD.md](PROJECT_WIZARD.md)) |
| Floor View Manager panel | Not built |
| Preventing edits of an inactive floor opened directly | Not built (the manifest records `locked`; opening the file still allows editing) |
| Adding or removing a floor after setup | Not built |
| Opening schedule: door/window can place a plan block and sync schedule width from the drawing | Built (`wall.door` / `wall.window` `block`, `openings.sync`); full Type catalog / reconciliation report still not built |
| Floor-wise electrical schedules and cross-floor coordination | Not built |
| Batch print by floor and category | **Removed** (`plot.batch` was deleted with paper space, H6; [layout-book.md](layout-book.md) 11.1) |
