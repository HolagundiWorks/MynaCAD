# Importing and maintaining drawings

Everything here is implemented and covered by tests. Limits are listed with each tool.

## Flatten on import

Every drawing opened from a file (`open`, drag and drop, `recover`) is flattened to 2D before it
is shown. The code is `crates/engine/src/flatten.rs`; the hook is `cmd::file::import_drawing`.

### What happens

1. The file is read by the DXF/DWG reader (`crates/io`). The reader already projects what it can:
   a 3D polyline becomes a 2D polyline, a `3DFACE` becomes a closed outline, and polyface and
   polygon meshes are skipped.
2. `flatten_drawing` runs on a working copy of the result. For every entity in model space, every
   layout and every block definition it sets z coordinates, elevation and thickness to zero (the
   same operation as the `FLATTEN` command, `modify2::flatten_kind`).
3. The drawing as read is kept as the document's *saved* state and as one undo step labelled
   "Flatten on import". The file on disk is not touched. Undo restores the original elevations
   and the drawing then counts as unmodified.
4. If anything was flattened or could not be, a report is written to the command line and returned
   in the `flatten` field of the `open` and `recover` results (and `Session::last_flatten`).

A drawing that is already flat opens unchanged: no undo step, no message, not marked modified.

### The report

| Field | Meaning |
|---|---|
| `examined` | entities looked at |
| `flattened` | entities whose coordinates, elevation or thickness changed |
| `unsupported3d` | 3D-only objects kept as read, by DXF type (`3DSOLID`, `BODY`, `MESH`, the surface types, `LIGHT`, `CAMERA`, `SECTION`) |
| `unmodelled` | other object types MynaCAD does not model, kept as read, by DXF type |
| `tiltedPlanes` | entities whose extrusion direction is not parallel to Z; their z values are zeroed but the plane is not projected |

### Limits

- 3D solids, surfaces and meshes are not converted; they stay in the drawing as raw data and are
  reported. They are not drawn or editable. Saving writes only the object types MynaCAD models,
  and says which it could not write (`notSaved`).
- Polyface and polygon meshes are dropped by the reader, not reported by the flattener.
- Hidden-edge flags of 3D faces are not kept.
- An entity on a tilted plane is not rotated into XY; only its z values are removed.
- Flattening can be run again on the whole drawing with `flatten {"all": true}` and on a selection
  with `flatten`.

## Backup before overwriting

The first time a save replaces an existing file, the old file is copied to `<name>.bak` (for
example `plan.dxf.bak`). The backup is made once and never replaced by later saves, so it stays the
file as it was before MynaCAD first overwrote it. If the backup cannot be written the save is
refused. On the web build, which has no file system, saving downloads a new file and nothing is
overwritten.

## PURGE

`PURGE` (alias `PU`, `-PURGE`) removes unused named definitions: blocks, layers, linetypes, text,
dimension, multileader and table styles, groups, and optionally zero-length geometry and empty text.

- Input: `{type?: all|blocks|layers|linetypes|dimstyles|mleaderstyles|tablestyles|textstyles|groups|zerolength|emptytext, names?: "A*,B"}`
  (default: every type, every name).
- Output: `{purged: {<type>: [names removed]}, message}`.
- Typed at the command line it asks for the type and the names and whether to verify each one, as
  `-PURGE` does. The JSON form purges without asking.
- Safety: one undo step restores everything. Definitions that are in use, layer `0`, `Defpoints`, the
  current layer, and the `Standard` and current styles are never removed.

## AUDIT

`AUDIT` finds and repairs database errors (`crates/engine/src/audit.rs`).

- Input: `{fix?: bool}` (default `false`: report only).
- Output: `{errors, fixed, issues: [{kind, location, handle?, message, fix}], message}`.
- Typed at the command line it reports first and asks before fixing.
- Repairs do not invent content: a missing layer or block is recreated with default properties
  (an empty block); a reference to a missing style or linetype becomes `Standard` or `ByLayer`;
  references to objects that do not exist are dropped; objects whose geometry cannot be drawn are
  removed; self-referencing blocks lose the reference that closes the loop; duplicate handles get
  new ones.
- Safety: with `fix` the repair is one undo step; the file on disk changes only when the drawing
  is saved, and then `<name>.bak` holds the original.

## RECOVER

`RECOVER` opens a damaged DXF or DWG file as a repaired copy in a new tab.

- Input: `{path}` or `{data: base64, name}`.
- Output: the audit result plus `index` (the new tab) and `flatten` (the import report).
- It reads the file, flattens it like any import, then applies the audit repairs. If the file
  cannot be parsed at all, it reports the reader's error and opens nothing.
- Safety: the source file is never modified. The repairs are unsaved changes; saving over the
  source creates `<name>.bak` first.
- Limit: recovery is only as good as the reader. A file the DXF/DWG reader rejects outright cannot
  be recovered.

## Tests

`crates/engine/tests/flatten_import.rs` (flatten, undo, report, `.bak`), `crates/engine/tests/audit.rs`
(audit and recover), `crates/engine/tests/purge_prompts.rs` and the purge unit tests, and
`crates/io/tests/entity_props_roundtrip.rs` (reader projection of 3D polylines, `3DFACE` and meshes).

## DWG files are read in a worker process

DWG parsing is done by a third-party library that can abort the whole process on a damaged file
(an impossible allocation cannot be caught) or run for a very long time. The applications therefore
read DWG in a short-lived copy of themselves (`aad_dwg::isolated`, flag `--aad-dwg-worker`): a
crash or hang is an error message, and the open drawings survive. The first thing `main` does in
`aad` and `aad-cli` is call `run_worker_if_requested()` and `enable()`. Anything else that
links `aad-dwg` reads in-process unless it does the same. The worker has a 180 s limit and its
memory is not capped. See BUG-001 in [BUG_BACKLOG.md](BUG_BACKLOG.md).

## Local demo drawing

`--sample` opens the built-in sample. If a file named by `AAD_SAMPLE_FILE`, or `sample/DEMO.dwg` / `sample/DEMO.dxf` beside the program (for the WinUI shell: `target/aad-winui/sample/`), exists, it is opened instead, as an unsaved copy: saving asks for a name and never overwrites it. The file is not in the repository (drawings made with other software are not committed). Checked by opening a copy of the OSAS demo DWG in the shell; there is no automated test for it.
