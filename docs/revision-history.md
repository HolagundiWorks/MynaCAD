# Version history and revision register

Tren keeps a **version history** of drawing stores (levels and sheet-set annotation) in the project file (or a sidecar `.history.osp` when the drawing is not a project), and a **revision register** in the drawing’s project data so sheets and the DXF/DWG carry issued codes even when history is missing.

## Files

| Item | Location |
| --- | --- |
| History database | `history_record` table in the open `.osp`, or `<drawing>.history.osp` beside a standalone drawing |
| Drawing identity | Project key `history` → `{ "drawingId": "…" }` |
| Revision register | Project key `revisions` → codes style, next counter, `entries[]` |
| Revision table ownership | Project key `revisionTableOwned` (per-layout handle lists) |
| Checksum sidecar (unchanged) | `<file>.aadrev.json` via `drawing.revision` |

The history file uses record kinds `0x40`–`0x45` so it never clashes with take-off kinds 1–9. Object and definition rows are content-addressed and shared across milestones.

## Commands

| Command | Role |
| --- | --- |
| `history.save` | Named milestone (`scope`: `current` \| `level` \| `sheet` \| `project`) |
| `history.list` | List milestones (newest first) |
| `history.restore` | Replace (default) or `mode: "copy"`; one undo step; auto “Before restoring …” point first |
| `history.rename` | Name an auto or named restore point (promotes `auto` → `milestone`; refuses revisions) |
| `history.delete` | Delete a non-revision milestone |
| `history.prune` | Drop old automatic points (default keep 20 + one per day for 30 days) |
| `history.compare` | Diff `from` vs `to` (milestone id or `"now"`): added / removed / changed with boxes |
| `history.preview` | Overlay the diff of a milestone vs now (added blue, removed dashed red, changed amber); `{off: true}` or Esc ends it |
| `history.mirror` | Not available yet (office shared history); returns a clear error |
| `revision.issue` | Snapshot as kind `revision`, append register, set title-block `REV`, redraw tables |
| `revision.list` / `revision.set` / `revision.void` | Read / edit text / void (milestones stay; void cannot be deleted from history) |
| `revision.table` | Draw/replace the table on `A-ANNO-REVT` |
| `revision.codes` | `letters` (default), `numbers`, or `custom` |
| `revision.clouds` | Cluster compare boxes within `gap`; returns `proposals`, or with `accept: true` places `revcloud` entities on `layer` (default `A-REV`) |

Automatic restore points run after a successful DXF/DWG save. A failure to write history is echoed and never fails the save.

## WinUI

- **History** side-rail panel (Window > History / File > History > List…): day-grouped timeline, “Named versions only”, click to preview, Restore, Restore as copy, Compare with now, Name this version, Delete, Save milestone. Esc ends preview.
- **Revisions** side-rail panel (Window > Revisions / File > Revisions > Register…): register grid with issue (description, by, checked, code, status), Update, Void, Draw table, Propose clouds (propose then accept).
- Level tab and sheet-set rail right-click: **Save milestone…** and **History**.

Programmatic `engine.execute` never opens those panels.

## Not in this release

- Office mirroring of history or take-off tags per revision (`history.mirror` refuses with a clear message).
- Delta tag blocks on revision clouds; per-proposal accept/drop UI (API supports propose then `accept`; the panel accepts all proposals in one step).

## Tests

- `aad-vdb` unit tests in `crates/vdb/src/history.rs`.
- Engine integration: `crates/engine/tests/history_revision.rs` (save/restore, shared rows, issue/void/DXF register round-trip, table replace, hostile file, revision lock, compare, preview/Esc, clouds, mirror refusal).
