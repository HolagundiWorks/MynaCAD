# Layer mapper

How the layers of a drawing that did not come from MynaCAD are put on MynaCAD's built-in layers. The
capability row is in [status.md](status.md); the geometry database's use of it is in
[GEOMETRY-DB.md](GEOMETRY-DB.md) (section 9).

## Rules

- **The standard layer set is fixed.** Only built-in layers are destinations (below). A name that is not
  in the catalogue is refused; `create` no longer makes a layer of the caller's own name.
- **Several source layers may go to one destination.** A source is read before anything moves, so a map
  that swaps two layers does what it says.
- **Nothing changes without a review.** The wizard lists the layers with a suggestion for each, then a
  review page (the engine's `preview`) with the counts, then Apply. No automatic apply.
- **The imported names are kept.** The source layer name of every moved object is recorded in the project
  entry `layerSources` (saved with the drawing in DXF/DWG extension data and in the `.osp`), so the
  names stay after the emptied layers are purged. The Properties panel shows `Source layer` for a single
  selected object that was moved.
- **Geometry and marks stay.** Only the layer of an object changes. Take-off marks, building-element
  classification, handles, colour, linetype and every other property stay on the object. Objects of
  every level, the layouts and the block definitions are moved. Objects on locked layers stay and are
  counted as `skippedLocked`. One undo step.
- **Programmatic calls never open a dialog.** `layermap` is JSON in, JSON out. The WinUI shell opens the
  wizard.

## Built-in destinations

`destinations` in `layermap {suggest: true}`, from `crates/engine/src/cmd/layermap.rs`:

- the standard architectural set, `STANDARD_LAYERS` in `crates/engine/src/cmd/hcw.rs`, and the seven
  `MAPPER_LAYERS` below;
- every layer the five discipline tabs create (`sheetstd.rs`: `S-RCC`, `S-STEEL`, `E-BOARD`, `P-WATR`,
  `FC-BOUN`, `M-HVAC`, ...);
- `S-BEAM` (the beam target of the geometry database, `EXTRA_LAYERS` in `geodb.rs`);
- the take-off layers `MEASURE-*` (not the label and table output layers).

Each destination has a group (Architecture, Annotation, Interior, Electrical, Plumbing, Site and
structure, False ceiling and HVAC, Presentation, Take-off), a colour, a linetype, a lineweight and a
description. A destination missing from the drawing is created with that look when a map uses it.
The permit (`BP-*`), presentation (`G-*`) and VH (`A_WALL_CUT`, ...) sets are built-in layer sets but are
not destinations.

### Layers added for the mapper

Seven layers were added as `MAPPER_LAYERS` in `layermap.rs`, beside the 36-layer standard set of the HCW
toolkit port (`STANDARD_LAYERS`), which keeps its size, numbering and tests. They are the places an
imported drawing's geometry most often had no standard layer for. They are created when a map uses them
(not by `hcwlayers`), count as built-in for origin detection, and are destinations:

| Layer | Use |
|---|---|
| `A-ROOM` | Room outlines, area boundaries and room labels |
| `A-ELEV` | Elevation linework and facade outlines |
| `A-SECT` | Section linework and cut outlines |
| `A-LIFT` | Lift shafts, cars and machine rooms |
| `A-EXIST` | Existing elements that stay (`A-DEMO` is for those to be demolished) |
| `S-FOOT` | Footings, foundations and plinth beams |
| `AN-TBLK` | Title block, sheet border and drawing frame |

They follow the discipline-prefix naming of the rest of the set (`A-` architecture, `S-` site and
structure, `AN-` annotation).

## Suggestions

`layermap {suggest: true}` returns, for each layer with objects that is not built in, `suggested` (a
built-in layer or null) and the `reason`. The words in a name decide: hatch, text and dimension win over
the general words (`WALL-HATCH` is a hatch); then the keyword table in `sheetstd.rs` (`AR_WALLS` ->
`A-WALL`, `ST_COLUMNS` -> `S-RCC`); then elevation, section, lift, foundation, existing, room and
border/title words. Quantity take-off layers (`MEASURE-*`) get no suggestion. Nothing is applied by the
suggestion. The layers of every level are counted.

## Command

`layermap` (Format > Layer Tools > Map Layers):

| Call | Result |
|---|---|
| `{suggest: true, only?: [names]}` | `{origin, needsMapping, layers: [{name, objects, suggested, reason}], destinations: [{name, group, description}], suggestions}`; changes nothing. `only` limits it to the layers an import brought in |
| `{map: {"WALL-A": "A-WALL", ...}, preview: true}` | `{preview, mapped: [{from, to, moved, locked}], total, skippedLocked}`; changes nothing |
| `{map, purge?: bool}` | `{moved, purged, skippedLocked, mapped}`; moves the objects, records `layerSources`, sets `layerMapDone`; `purge` removes each source layer left empty (not 0, Defpoints, the current layer or a locked one) |
| `{sources: true}` | `{sources: [{source, dest, objects, present, truncated}]}`: the recorded source names |
| `{dismiss: true}` | The drawing keeps its layers and the wizard is not offered again (project key `layerMapDismissed`) |

Refused without changing anything: a missing source layer, layer 0, Defpoints, an xref layer (`|` in the
name), a destination that is not built in or not a name, an empty map, the same source mapped to two
destinations, a bad `only`. Names are matched without regard to case; `wall-a` and `WALL-A` are one
layer.

`needsMapping` is true when the drawing did not come from MynaCAD (no `trenOrigin` stamp, see
GEOMETRY-DB.md section 9), the mapper has not run, nobody dismissed it, and layers with objects remain
that are not built in. An empty drawing, a drawing on layer 0 only and a drawing that already uses only
built-in layers never need mapping. With `only`, the foreign check is skipped: the named layers are
offered even in a MynaCAD drawing.

## Recorded source names

Project entry `layerSources`: `{version: 1, entries: [{source, dest, objects, handles?, truncated}]}`.
`handles` (hex) lets a single object be traced back; at most 50 000 per entry (then `truncated`), at most
4 096 entries. `level.import` with a `layerMap` records source, destination and count without handles
(the imported objects get new handles). Mapping the same source to the same destination again adds to the
entry.

## Where it is offered

| Path | Behaviour |
|---|---|
| Open a file (WinUI: File > Open, Welcome list, command-line files) | After the file is open the shell calls `layermap {suggest}`; when `needsMapping` is true the **Map Layers** window opens |
| File > Import of a DXF/DWG | `import` reports `layers` (the file's layers that are not built in); the shell opens the window for those |
| Architecture > Levels > Import Level | The existing per-file layer window (`layer.mapplan`, `level.import {layerMap}`); `layerMap` targets must now be built in |
| Layer manager: select layers, right-click > Map to built-in layers... | The same window limited to the selection |
| Format > Layer Tools > Map Layers, or `layermap` on the command line | JSON only |

The window: one row per layer (name and object count) with a built-in layer chosen from a list (the
suggestion preselected, or "keep this layer as it is"); **Review** shows what moves; **Apply** runs the
map with the option to remove the emptied layers; **Back** returns; **Keep my layers** (offered on open
only) dismisses it for this drawing; **Cancel** closes it and changes nothing, so the drawing is offered
the window again the next time it is opened. Open from the control channel (`app.open`) never opens it.

## Verification

Engine tests in `crates/engine/tests/layers_levels_workflows.rs` (the mapper section) and the layer map
test in `crates/engine/tests/migrate.rs`. The wizard window and the Layer manager multiple selection are
WinUI code that compiles; see the verification notes in the report of this change and the status row.

## Not done

- A saved named mapping per company or per source program; matching by block names, colours, linetypes
  or what the objects look like; layers that mix classes (everything on layer `0`).
- Mapping of linetypes, text styles and the layers inside xrefs.
- The mapper is not offered for PDF, SVG or picture imports (they arrive on their own layer), nor in the
  egui shell.
- Block definitions are retargeted but their objects are not listed in `layerSources` by handle.
