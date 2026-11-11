# Building elements, page layouts and print blocks: documentation against the code

Checked on 2026-11-07 against `main`. The document read is [building-elements.md](../building-elements.md)
(the BIM-style classification note); the page layout side was read from [sheets.md](../sheets.md),
[levels-and-sheet-sets.md](../levels-and-sheet-sets.md) and [status.md](../status.md). Each claim below was
checked in the code or by running a command, not taken from another document.

## 1. building-elements.md: what holds

| Claim | Result |
|---|---|
| `classify.set`, `classify.get`, `classify.clear` | Registered (`cmd/classify.rs`) |
| `spec.list`, `spec.get`, `spec.search` and `assets/specs/indian-index.json` | Registered; the index has 18 entries, titles and ids only |
| 12 kinds: column, beam, footing, wall, slab, plaster, paint, flooring, door, window, grill, other | Exactly those (`doc/src/building.rs`), each with a default IFC class |
| Fields `family`, `kind`, `specRefs`, `ifcType`, `measurementRule` | Present on `BuildingElement` |
| Groups nest to depth 16; `group.explode` | `MAX_GROUP_DEPTH = 16`; command registered |
| `component.make`, `place`, `makeunique`, `edit` | All registered (`cmd/component.rs`) |
| Quick Properties shows Building kind and IFC type, writing through `properties.set` | Present (`aad-host/src/panels.rs`) |
| Bridge and take-off rows carry `ifc_type`, `building_kind`, `building_family`, `spec_refs`, `measurement_rule` | True for the wall, opening, column, lintel, room and slab rows (`stamp_building_on_row`, 6 call sites in `bridge.rs`) |
| No `.ifc` export from MynaCAD | True: MynaCAD stores IFC class names as metadata only |

## 2. building-elements.md: what is missing or has drifted

1. **Toolbar symbols are never classified.** The RCC, structural, electrical, plumbing, interior and HVAC smart
   blocks carry no `BuildingElement`, so a column placed from the toolbar has no IFC class, kind or measurement
   rule until someone runs `classify.set` on it. `smartblock.rs` does not mention classification.
2. **The electrical and plumbing take-off rows are not stamped.** Kinds 10 to 13 (switchboard, circuit,
   plumbing source, plumbing fixture) skip `stamp_building_on_row`, so they have no `ifc_type` or `building_kind`
   although the walls and columns beside them do.
3. **The kind list has no services or circulation.** There is no pipe, fitting, fixture, cable, light or socket
   kind, and no stair, roof, railing or furnishing. Pipes and fittings (added 2026-11-07) therefore cannot be
   classified except as `other`.
4. **Two taxonomies.** Myna3D (Myna3D) has 16 IFC classes in `bim/taxonomy.py`, including stair, roof, railing,
   member, plate and furnishing, and an IFC4 exporter (`bim/ifc4.py`). MynaCAD has 12 kinds, including footing,
   plaster, paint, flooring and grill, which Myna3D lacks. Nothing keeps the two lists in step.
5. **The note is silent on smart blocks and names a type that is not the plan.** "Parametric types beyond
   `osas.wall` (`osas.column`, etc.)" is listed as not in this release, but columns, beams and footings now exist
   as smart blocks (`rcc.*`, `struct.*`). `osas.wall` is still the only `param` component type.
6. **Myna3D still points at ShilpiDB.** `docs/developer/bim-shilpidb.md` and `bim/shilpi_client.py`,
   `bim/shilpi_ifc.py` describe the retired ShilpiDB server; the project file replaced it.

## 3. Page layouts and print blocks: what holds

| Claim | Result |
|---|---|
| A region of the model is named with `printblock`: a closed boundary on layer `PRINT-BLOCK`, a name, a description from the level and tab | True (`cmd/printblock.rs`) |
| Listing a print block stores that description so MynaOps can read it | True: `printblock.list` writes it into the print block book (the `Item.description` field says so) |
| Paper layouts are not tabs | True (status bar and tab code) |
| Every `sheet.*` command is documented | True; all 18 are mentioned |
| Compose Views, Master Layouts and Publisher Sets (`compose.view.*`, `master.*`, `publisher.*`) | Documented in `levels-and-sheet-sets.md`, as wildcards, and implemented (5, 5 and 6 commands) |
| Revision register and revision tables (`revision.table`) | Implemented and documented in `revision-history.md` |

## 4. Page layouts and print blocks: what is missing or has drifted

1. **`sheets.md` has a broken paragraph.** Two lines after the print block sentence read "...so MynaOps can read /
   for the project-aware ones); the Plot window only calls them." Text between those two lines was lost, so the
   description of MynaOps's Outputs > Layout flow is cut off in the middle of a sentence.
2. **`sheets.md` says revision tables are not implemented.** Its "Not implemented" list starts "Revision tables,
   live sheet index, ..." but `revision.table` exists and `revision-history.md` documents it. The live sheet index
   and templates that carry viewports are correctly listed as not done.
3. **The Page Layout Manager and General Notes windows are not documented anywhere.** `page.manager`,
   `page.toggle`, `notes.manager`, `notes.std`, `notes.list` and `notes.apply` were added on 2026-11-03 and no document in
   `docs/` names them (`notes` itself appears only in passing).
4. **`printblock.rs` contradicts itself.** Its module comment says "The description is not stored. It is the
   current name of the level and the tab", but `printblock.list` stores it (and `sheets.md` is right).
5. **The individual `layout.*` commands (`layout.new`, `copy`, `rename`, `set`, `list`) are not named in any
   document**, and `printblock.list`, `printblock.set` (rename) and `printblock.delete` are described only in the
   command text, not in `sheets.md`.
6. **Print blocks are outside the classification and take-off model.** A print block is a named boundary; it has
   no `BuildingElement`, and it is not carried in the take-off record. MynaOps reads it from the project file
   directly. That is consistent, but no document says where the line is.

## 5. Suggested order

1. Repair `sheets.md` (item 4.1) and correct its "Not implemented" list (4.2): both are quick, and the first one
   hides part of the MynaOps hand-off.
2. Document the Page Layout Manager, General Notes and the `layout.*` and `printblock.*` commands (4.3, 4.5).
3. Fix the `printblock.rs` module comment (4.4).
4. Classify toolbar symbols on insert, with the default kind and IFC class in each smart block definition, and
   stamp the electrical and plumbing rows (2.1, 2.2). This is the change that makes the classification useful.
5. Extend `BuildingKind` for services and circulation, and agree one list with Myna3D (2.3, 2.4).
6. Update `building-elements.md` for smart blocks (2.5) and retire the ShilpiDB pages in Myna3D (2.6).
