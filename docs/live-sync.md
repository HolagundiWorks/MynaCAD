# Live sync of the shared project file (`.osp`)

Status: **MynaCAD side (stage 1), MynaOps side (stage 2) and a hand run of both apps (stage 3) are done; see "Build plan and verification status" and "Run by hand" for exactly what was verified and what was not.**
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
| 1 | `aad-odp`: `sync.last` and `sync.takeoff.*` in the commit, read-only `sync::probe`; engine `project.sync` (control channel, host method) with accepted-revision tracking; WinUI status text `SyncState` | Rust tests pass (odp 31, engine `project_sync` 4 + 3 unit); fmt, clippy `-D warnings`, `cargo xtask layers` clean; WinUI shell builds; status text read in a window (see below) |
| 2 | MynaOps (`Office.Shared`: `OdpSync`, `SyncStatusService`, `ProjectFileWatcher`; `sync.last` in `OdpFile.Write`; status text in the status strip; Take-off page accepts on refresh and shows the note) | 11 `OdpSync` xunit tests pass; `BBSApp` builds (Release x64) |
| 3 | Both apps on one test `.osp` | see below |

The existing MynaOps 2 s poll (`ServedProject.PollAsync`) is reused: the status refresh runs inside it and the watcher only calls it earlier. The ops-level-ui `level_status.stale` header flag (fingerprint per level) is separate from the file-level stamp stale marker described above; MynaOps does not yet merge the two.

### Configuration in managed mode

A MynaCAD started by MynaOps also reads the `config.*` keys on a timer of its own (about every 3 s, offset from the `project.sync` probe by half a period) and applies a newer `config.rev` when the shell is idle: `project.config.poll` in `aad-host`, which calls `project.config.apply`. It never reloads the drawing. The first poll of a run always applies (idempotent), whatever `config.applied.cad` says. Details and what was run: [ops-first-workflow.md](ops-first-workflow.md) section 12.

### Run by hand

Run on 2026-11-15 on the merged branches (MynaCAD `claude/osp-live-sync-aa35c6`, MynaOps `live-sync`, both with `main` merged). MynaCAD WinUI shell (debug build, `--control 47911`) and MynaOps (Release build) open on one test file `house.osp` (a drawing with one brick wall, saved with `saveas`, which records the take-off). Each app's status text was read through UI Automation (`ProjectSyncStatus`); MynaCAD's `project.sync` was also read over the control channel. Times are local.

| Step | Seen |
|---|---|
| Both apps open | MynaCAD `Synced`, MynaOps `Synced` |
| MynaCAD adds a wall and runs `takeoff.record` | MynaCAD stays `Synced` (own write); MynaOps shows `Updated by MynaCAD at 10:06` at the first read, 0.8 s after the record returned, and keeps it |
| A second writer (a Python sqlite script standing in for MynaOps: writes `project_doc`, bumps the revision, sets `sync.last` with app `mynaops`) | MynaCAD `Updated by MynaOps at 10:06` within 1 s; MynaOps also shows `Updated by MynaOps` because the writer was not MynaOps's own code path (a real MynaOps save is accepted by `NoteOwnWrite`) |
| Another process holds the file exclusively for 9 s | MynaCAD `Locked`, MynaOps `Locked` (MynaOps flickered back to `Updated` between two polls); both returned to `Updated by MynaOps at 10:06` after the lock ended |
| A copy of another project (other project id, lower revision) copied over the open file | both apps `Project file was replaced` within 4 s |
| The file overwritten with text | both apps `Project file unreadable` (`file is not a database`); both stayed responsive |

Not verified: a real MynaOps File > Save raising a revision (Ctrl+S sent to the window wrote nothing in an earlier attempt; the xunit test covers `sync.last` on `OdpFile` writes); the Take-off page reloading and accepting the file, and the level picker keeping its selection (the nav item could not be selected through UI Automation); the status tooltips; watcher latency apart from the poll; MynaCAD saving after a `replaced` state (it still writes only its own tables and the shell tooltip tells the user to reopen); a restore or Compact done from the MynaOps Project database page. The MynaCAD shell exited once in an earlier run (cause not found); it did not in this run.

### Seen with the real apps (Ops-first package F, 2026-11-16)

With MynaOps and MynaCAD both running on one project: a configuration revision written by MynaOps was applied by MynaCAD about 3 s later; a take-off recorded by MynaCAD appeared in MynaOps' per-level summary after the Take-off page was opened; a lock held by another process showed `Locked` in both; the file overwritten in place by another project's file showed `Project file was replaced` in both and the open drawing was not reloaded. Details and the defects found: [ops-first-workflow.md](ops-first-workflow.md) section 14.

### Realtime sync on/off and Sync now (Ops-first package D)

The detector described above is the **Realtime sync on** behaviour. Each app has its own switch (state per run, on at start, not stored in the file). **Off** stops every read of the file by the detector on that side (MynaCAD: the 3 s `project.sync` probe and the `project.config.poll`; MynaOps: the 2 s poll, the watcher-driven poll and the status refresh) and applies nothing; the text becomes `Sync off`, with `n changes waiting` (revisions the file is ahead of the accepted one) from one probe taken only when the user looks. **Writing is never paused.** **Sync now** flushes unsaved work with the normal save, then makes one forced refresh (MynaCAD also applies a pending configuration), whatever the switch says. MynaOps can set and trigger MynaCAD's through the activation pipe (`sync.set`, `sync.now`, `status`). Sync now also accepts the file as it is now, so nothing stays waiting. Contract, acceptance and what was run with the real apps: [ops-first-workflow.md](ops-first-workflow.md) section 15.
