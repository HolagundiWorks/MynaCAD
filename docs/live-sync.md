# Live sync of the shared project file (`.osp`)

Status: **stage 1 (MynaCAD side) implemented and tested; MynaOps side and the two-app run are tracked in the sections marked as such below.**
Scope: two programs on one computer (MynaCAD and MynaOps) that have the same `.osp` open. No network, no MynaHub, no Neon (that sync is a
separate, paused MynaHub matter). The format stays **additive**: only new `meta` keys, no new table, no changed column.

"Live sync" is read here as: when one app writes the shared project file, the other app notices within a few seconds, says so, and refreshes
the data it shows without disturbing what the user is doing. If the owner meant something else (for example multi-user editing of one
drawing, or cloud sync of the project between computers), this is not that; those are the Office-history mirror and the MynaHub sync in
[status.md](status.md).

## What exists today (read before this work)

| Piece | Where | Behaviour |
|---|---|---|
| Revision counter | `aad-odp` `Edit::commit`, MynaOps `OdpFile.Write` | Every committed change raises `meta.revision` by one and leaves `change` rows. Local-only data (native model, local `meta` keys, history) is committed without a revision |
| SQLite mode | `Odp::tune`, `OdpFile` | WAL, `synchronous=NORMAL`, 5 s busy timeout, writes use `BEGIN IMMEDIATE`: one writer at a time, readers never block |
| MynaOps poll | `Services/Odp/ServedProject.PollAsync`, every 2 s | Reads `meta.revision`; a revision this app did not write raises "MynaCAD updated the take-off". Own saves are excluded with `NoteOwnRevision` |
| MynaCAD | none | Never looked at the file after open; no "changed elsewhere" notice |
| MynaCAD writes | `write_project` (save: drawing, structural details, native model), `OdpSink::save` (`takeoff.record`: header, rows, annotated PDF) | MynaCAD autosave writes only to the recovery folder, never the `.osp` ([AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md)) |
| MynaOps writes | `SetProjectDoc` (File > Save: the project record) | Nothing else in production code (`SetTakeoffRow` / `SetDrawing` are used by tests) |
| Replace under a running app | MynaOps Project database: Compact (`VACUUM`), Restore (rename a backup over the file) | Both are refused while the file is open elsewhere (active-project pointer, handle check) |

## Who owns what (single writer per table)

| Data | Writer | Reader | Flows |
|---|---|---|---|
| `drawing`, `drawing_state`, native model (`native.*`, parts), `detail`, history | MynaCAD | MynaOps (drawing, details), viewer | CAD → Ops |
| `takeoff_header`, `takeoff_row`, annotated PDF assets (`annotated-<slot>.pdf`), level ids and the header `levels` list | MynaCAD (`takeoff.record`) | MynaOps | CAD → Ops |
| `project_doc` (estimate, rates, running bills, floors, project details) | MynaOps | MynaCAD does not read it today | Ops → CAD only where already shared: the floor list and project details travel through the existing **Export for CAD / Import from CAD** file, not through the `.osp`; live sync does not change that |
| `meta.sync.*` (below) | Whoever wrote the revision | Both | Both |

Rules that follow from this:

1. **A program writes only the tables it owns.** MynaCAD never writes `project_doc`; MynaOps never writes `takeoff_*`, `drawing`, `detail`. So a normal write by one app can never overwrite a record the other app edited, and there is no merge. A program that would have to write a table it does not own refuses.
2. **Never reload under the user.** The program that notices a change tells the user; it does not replace what is on screen unasked. MynaCAD only reports; reopening is the user's File > Open (or Reload). MynaOps refreshes read-only views (the Take-off page's data and levels) and keeps the selection; it never discards unsaved edits in the project record.
3. **Detection is read-only.** The detector opens the file read-only, reads a few `meta` keys, closes it. It takes no write lock and cannot change the file.

## Change detection

| Option | Used? | Why |
|---|---|---|
| `meta.revision` poll | **Yes (the signal)** | Already the contract in both apps; one indexed read; survives the WAL and works the same from Rust and .NET |
| `PRAGMA data_version` | No | Only changes for writes by *other connections of the same open handle*; a read-only connection opened per poll starts from a fresh value, so there is nothing to compare |
| File watcher | **Yes, as a hint only (MynaOps)** | `FileSystemWatcher` on the file and its `-wal`, with a 300 ms debounce, brings the poll forward from "up to 2 s" to "about half a second". A watcher can miss events (network drives, buffer overflow, atomic replace), so the 2 s poll stays as the fallback |
| MynaCAD side | Poll | `project.sync` is called by the shell about every 3 s and by the control channel on demand; the host adds no thread |

Because local-only writes do not raise the revision, the stamp described next is written inside the same transaction as the data it describes
(in `commit`), never separately, so a reader never sees a revision without its stamp.

## Sync keys in `meta` (additive)

All values are JSON text. Keys use the prefix `sync.`; readers ignore keys they do not know; the format version does not change.

| Key | Written by | Value |
|---|---|---|
| `sync.last` | every revision-raising commit of either app | `{"app":"mynacad"\|"mynaops","revision":N,"time":"2026-11-15T09:30:12Z"}`: who wrote revision N and when. MynaCAD writes it from `Edit::commit` (so also for the drawing save); MynaOps writes the same key from its `Write` |
| `sync.takeoff.<slot>` | MynaCAD, on every `takeoff.record` (also when nothing changed: "the take-off matches the drawing as of revision N") | `{"revision":N,"recorded":"<utc>","levels":{"<level id>":{"revision":N,"recorded":"<utc>"}}}` — `N` is the file revision the record reflects; `levels` lists the level ids that have rows in that slot (from the header `levels` list; empty for a drawing with one implicit level) |

The task brief said "header keys"; the stamps live in `meta` instead of the take-off header JSON. Reason: the header is compared byte for byte by
`replace_takeoff` to decide whether a record changed anything, so a recording time in it would make every save a new revision.

### Stale take-off marker

A take-off is **stale** when the drawing was saved after it was recorded: `drawing.revision > sync.takeoff.<slot>.revision` (the `drawing.revision` column is
set on every `put_drawing`). Because MynaCAD records on every save that has take-off data, this is false after a normal save and true when
a save happened but the record was skipped or failed (the save echoes "Take-off was not recorded"), or the drawing was changed by another tool. MynaOps shows the marker per level using the per-level stamp
(all levels of one recording share it today; a recording limited to one level, `takeoff.record {level}`, stamps that level only). Both
apps compute it the same way; neither writes it.

## What the user sees

One short text in each app, same words:

| State | Text | When |
|---|---|---|
| `synced` | `Synced` | The file's revision is the one this program wrote or last accepted, and no take-off is stale |
| `updated` | `Updated by MynaOps at 14:32` / `Updated by MynaCAD at 14:32` | A revision this program did not write is in the file (time local, from `sync.last`; without the key: `Updated elsewhere`) |
| `outofdate` | `Take-off out of date` | A take-off stamp is older than the drawing (above) |
| `locked` | `Locked` | The file cannot be opened right now (another program holds it; busy longer than the short probe timeout). Not an error: the next poll tries again |
| `replaced` | `Project file was replaced` | The project id changed, or the revision went **backwards** (a restore or a copy over the file) |
| `unreadable` | `Project file unreadable` | Not an `.osp`, damaged, partial, or newer than this program reads. The reason is in the tooltip / `project.sync` reply |
| `missing` | `Project file missing` | The path no longer exists |
| `none` | (nothing shown) | The drawing is not a project file |

MynaCAD: a status text beside "Saved hh:mm" in the status bar, the command `project.sync` (`{ack?: true}`), also on the control channel and MCP.
`ack` makes the current revision the accepted one (after the user has reopened the file or decided to ignore the change). MynaOps: the same text in the status bar and on the Take-off page header.

## Failure modes

| Situation | What happens |
|---|---|
| File locked / busy | The probe uses a 250 ms busy timeout and a read-only open. Result `locked`; shown as such; retried at the next poll. Never blocks the UI thread for more than that |
| Partial write | WAL commits are atomic: a reader sees the old or the new revision, never half. A file cut short is "unreadable" |
| Other app closed | The file stays as it is; the status stays `Updated by …` until acknowledged. No pointer or lock file is needed |
| File replaced (restore / compact / copy) | Detected by project id change or a revision lower than the accepted one → `replaced`. MynaCAD does not silently go on writing: a save after a `replaced` state still writes only its own tables (it cannot clobber MynaOps data), but the shell tells the user to reopen. Compact keeps revision and content: no change is reported (verified by design, not run against the real Compact) |
| Corrupt / not SQLite / hostile values | Any SQLite or parse error becomes `unreadable` with a reason; no panic, no allocation sized from the file (only short `meta` strings are read, capped at 4 KiB each) |
| No sync keys (file from an older program) | `Updated elsewhere` without a time; stale marker absent (no stamp, no claim) |
| Own writes | Never reported as "changed elsewhere": every MynaCAD write records the revision it made as the accepted one |
| Two MynaCAD instances | The second one's writes look like "Updated by MynaCAD" to the first; same rule applies |

## Build plan and verification status

| Stage | Content | Status |
|---|---|---|
| 0 | This document | done |
| 1 | `aad-odp`: `sync.last` and `sync.takeoff.*` in the commit, read-only `sync::probe`; engine `project.sync` with accepted-revision tracking; WinUI status text | see [status.md](status.md) |
| 2 | MynaOps: debounced watcher, refresh of Take-off page and levels keeping the selection, stale marker, status text, `sync.last` on its writes | see MynaOps `docs/bridge/LIVE-SYNC.md` |
| 3 | Run with both apps open on one test `.osp` | see "Run by hand" below |

### Run by hand

(Filled in by stage 3 with what was actually seen.)
