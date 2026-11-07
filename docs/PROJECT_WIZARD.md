# Project Setup Wizard, drawing sets, Xrefs and company profiles

This is the architecture audit and the state of the work for the Project Wizard specification. It
separates what exists from what is planned ([status.md](status.md) holds the overall status).

## Audit: what the repository already has

| Area | Where | State |
|---|---|---|
| Project folder, floor files, layout files, main drawing, manifest `project.aad.json` | `crates/engine/src/cmd/projfile.rs` (`project.setup`, `project.open`) | Implemented, tested. The manifest is untyped JSON (`serde_json::Value`) with floors and layouts keyed by name |
| Xrefs: attach, reload, detach, bind, overlay, relative paths | `cmd/xref.rs` | Implemented, tested. Reads a drawing into a block; no clipping or fading |
| Floors: activate, flags, overlays | `cmd/projfile.rs` (`floor.*`) | Implemented, tested |
| Project data in the drawing (details, floors, heights, schedules) | `cmd/project.rs`, `Drawing::project` | Implemented |
| Schedules and take-off | `cmd/schedule.rs`, `cmd/takeoff.rs` | Implemented; no opening *type* field, generated opening blocks or reconciliation report |
| Sheets and batch plot | `cmd/sheet.rs`, `plot.batch` | Implemented; no title block or nameplate with fields |
| File formats | `crates/io` | DXF read and write; DWG read. The project files Tren writes are DXF |
| Wizard UI, template manager, company profiles, nameplates, recovery | none | Not built (before this work) |

Constraints found: identities in `project.aad.json` are names, not ids (renaming a floor breaks its
links); there is no company concept; no title-block resource exists, so the nameplate has to be built
from existing entity types (blocks with attributes are supported by the drawing model, images are
not); generated drawings are DXF, so the default file extension is `dxf` and `dwg` is accepted in
the naming rules only for when DWG writing exists.

## What is built (steps 1, 2, 4 and 5 of the implementation sequence)

`crates/engine/src/projectplan/`, tested in `projectplan/tests.rs` and
`crates/engine/tests/project_plan_cmd.rs`:

- **Typed manifest** (`ProjectConfig`, schema version 1, `project.json`): project, floors, drawing
  sets, floor-by-set matrix, Xref rules, schedules, naming rules, generation status. Unknown or
  missing fields take defaults; a malformed, oversized or newer-schema file is an error message, not
  a crash. Stable ids (`new_id`), display names kept separate from ids and file names.
- **Floors**: heights and elevations are optional values; a missing one is reported, never
  defaulted. `resolve_elevations` derives `Z(n+1) = Z(n) + H(n)` for floors without an elevation,
  keeps a given elevation as it is, and rejects floors that are not above the one below.
- **Planner** (`plan`): expands a configuration into folders, drawings and Xrefs. Drawing ids are
  derived from project, set, floor and kind, so they survive renames and re-planning. File names
  and drawing numbers come from patterns (`{project}`, `{set}`, `{floor}`, `{seq:03}`). Xref rules
  (`host` selector, `target` selector, attach or overlay) are expanded into explicit edges with
  relative paths; nothing is inferred from file names. Checks: unique ids, names and codes, safe
  paths (no `..`, drive letters or reserved device names), file-name collisions, size limits, and
  Xref cycles (Kahn's algorithm, no recursion).
- **Preflight** (`preflight`): read-only report of the root folder and which planned files already
  exist, so generation can refuse to overwrite them silently.
- **Company profiles** (`projectplan::company`): typed profile, validation, `companies/<id>/company.json`
  with atomic writes, listing, load by id (never by name), managed logo asset (PNG, JPEG or SVG up
  to 5 MB), and `nameplate_lines` that leaves empty fields out. Offline / unlinked only: `save` and
  `set_logo` are refused when `OSAS_COMPANY` is set (suite-linked Draft must refresh firm identity
  from Studio via `companyfile`, not write a parallel local profile).
- **Generator** (`projectplan/generate.rs`, command `project.generate`): creates the folders, one
  drawing per planned drawing through the engine's own commands (units, project details, floors and
  an `aadDrawing` record holding the drawing id, number, title, set, floor and elevation), then
  attaches the Xrefs after all targets exist, then verifies that every file opens and every attached
  Xref resolves. Lifecycle `Validating → Generating → LinkingXrefs → Verifying → Ready`; any failure
  ends `Failed` and never `Ready`; the result is recorded in `project.json` (`generated`).
  Existing files are never overwritten and a kept drawing is never modified to add Xrefs, so
  regenerating after the user edited a drawing leaves it byte for byte as it was. An invalid
  configuration, a different project in the folder or a non-empty folder writes nothing. Overlay
  Xrefs are session-only in this engine, so they are recorded in the manifest and not written into
  the files. Tested on real files: `apps/aad-cli/tests/project_generate.rs` (generate, verify,
  move the folder, regenerate, refuse, interrupt, resume, clean up).
- **Recovery** (`generation.log.json`, command `project.recover`): the log is replaced atomically
  after every step and records the files this run created with a hash of their content and the
  files that may be half written. `inspect` reports what an interrupted run left; `cleanup` removes
  only recorded files that are still unchanged; `project.generate {resume: true}` continues and
  rewrites only the files the log marks as possibly half written.
- **Commands**: `project.plan` (validate and preview, writes nothing), `project.generate`, `project.check`, `project.recover` and `company.validate`.

## Decisions (approved)

1. **One manifest.** `project.json` is the single source of truth. `project.aad.json` is generated
   from it by `project.generate` (`projectplan/compat.rs`) for the floor commands, carries a `compat`
   block (`source: project.json`, a hash of what it was made from), and is never edited by hand.
   The mapping from the ID-based model is in the header of `compat.rs`: floor `code` is the legacy
   `key`, the plan set (code `AR`, else the first per-floor set) gives each floor's `file`, the other
   per-floor sets become `layouts`, the master drawing is `main`, and lengths are written in metres.
   The only state kept in the legacy file is what the floor commands change at run time (`visible`,
   `frozen`, `locked`, `print`, `active`); regenerating keeps it for every floor and layout that still
   exists. `project.check {root}` compares the file with `project.json` and reports a missing, damaged,
   hand-kept or stale copy. Generated drawings carry a `projectManifest` pointer so `floor.list`,
   `floor.set` and `floor.activate` work on them. Tested in
   `apps/aad-cli/tests/project_generate.rs`. Still to do, as its own step: move the floor
   commands to ID-based references; only then remove `project.aad.json` (the schema versions are
   `schema_version` 1 in `project.json` and `version` 1 plus `compat.version` 1 in the legacy file).
2. **File format.** DXF now. Generation goes through the engine's own commands and file writer, and
   the tests reopen the generated files. `project.json`, the ids, naming rules, floor relationships,
   schedules and company links do not depend on the format, so DWG can be added later by changing the
   extension and the writer. DWG writing is not claimed.
3. **Logo.** The logo is a managed asset of the company profile (`assets/logo.<ext>`, relative path
   recorded, `company::validate_assets` checks it exists) and a nameplate template may refer to it.
   Nothing draws it until the drawing model has an image entity (a separate engine capability:
   serialisation, rendering, relative paths, export). The nameplate text (`nameplate_lines`) is
   complete without it, and storing a logo changes none of its lines.

## Project Wizard for one drawing (built): new and edit

File > Project Wizard (`apps/aad-winui/ProjectWizardWindow.cs`, engine command `project.wizard`,
`cmd/wizard.rs`) sets up a new single drawing in five pages: the project (name, client, location,
site address, property ID, site area, plot use, authority, date, summary); the team and company
(architect, engineer, and a button that reads the Kivo company file); the building (drawing unit,
floors above ground, basements, terrace, a default floor-to-floor height and an editable list of
floor names and heights, slab and beam defaults, one level per floor or not); the standards
(architectural, permit and presentation layer sets, the five discipline tabs, symbol blocks); and a
review. Create runs one `project.wizard` call: `project`, `drawing.units`, `level.setup`,
`hcwlayers` and `sheetset.standard` in order, as one undo step; a refused part puts the drawing back
as it was.

**Edit.** The Project panel in the side dock is read only (labels and values, an "Edit project..."
button). The button, and File > Project Wizard on a drawing that already has floors or tabs, open
the same window as "Edit Project", filled in from the drawing. An edit changes the project details
(an empty field clears one), the slab and beam defaults, the layer sets, and adds standard tabs the
drawing lacks (a tab that exists keeps its mapping). The floors, levels and unit are shown but not
changed there: the engine refuses `units`, `floors` and `levels` on a set-up drawing, and the
wizard points to the level tabs (right-click) and Drawing Units. One undo step. This is separate from the folder-and-manifest wizard in the table below, which stays unbuilt.
Tested: `crates/engine/tests/project_wizard.rs` (whole flow and undo, skipping parts, refusals leave
nothing behind, hostile parameters). The window was driven by hand through all five pages and Create.
Not built in it: plot scale, per-floor elevations other than the running sum, templates and saved
drafts, the logo and several architects (the company file supplies neither).

## Not built yet

| Step | Item |
|---|---|
| 3 | Wizard UI (six steps, draft persistence, review tree and floor-by-set matrix), template manager (built-in, company, project snapshot; compare and apply updates) |
| 5 | Xref maintenance as a service (reload modified references, list the hosts of a file, repath after a move; the generator already attaches and verifies), per-set default layers and styles, DWG output |
| 6 | Opening schedule types, generated opening blocks, reconciliation of schedule and drawing instances, CSV export |
| 7 | Nameplate template with named fields, binding to company and project data, update policy (future drawings or linked nameplates), linked, overridden and frozen states |
| 8 | Xref maintenance (missing, reload, who references what, repath after a move), project packaging with a profile snapshot |
