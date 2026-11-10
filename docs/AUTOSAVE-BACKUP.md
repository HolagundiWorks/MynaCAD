# Autosave, crash recovery and backups

Status: **implemented** in the engine and the host with headless tests (including a host process killed in the middle of editing). The shell part
(status text, the restore question on start, File > Restore from Backup) is implemented in `apps/aad-winui` and compiles; it was not driven in a
running window, see "Not verified" at the end.

What existed before (and still does): a DXF or DWG written directly keeps the file it first replaces as `<file>.bak`, once; a save of a DXF or DWG
records a restore point in the drawing history (`<drawing>.history.osp`, pruned to 20 automatic points or 30 days) and a revision sidecar
(`<file>.aadrev.json`). There was no autosave, no recovery after a crash and no backup of a project file (`.osp`).

## What is kept, where, how often

| | What | Where | When | How many |
|---|---|---|---|---|
| **Recovery snapshot** | The unsaved state of each open drawing that has unsaved changes, as native-model parts ([NATIVE-OPEN.md](NATIVE-OPEN.md)) | `%LOCALAPPDATA%\OSAS\recovery\rec-<pid>-<n>\` (`AAD_RECOVERY_DIR` moves it; a folder left by a crashed run with the same name is never reused, the new one gets `-2`, `-3`, ...). Never in the drawing's folder, never in the drawing's file | Every `AUTOSAVESECS` seconds (default **120**) while there are changes not yet in a snapshot, and **10 s after the last edit** of a burst | One per open drawing. Removed when the drawing is saved or closed, or the program ends normally |
| **Backup** | The whole project file as it was before a save changed its drawing (or before a re-import or a restore replaced it) | `<name>.backups\<name> <UTC time> <reason>.osp` beside the project file (for a working copy in the cache folder: beside it there) | On every save that changes the drawing | The newest `BACKUPCOUNT` (default **5**); of those, any older than 90 days except the newest |

Settings: system variables `AUTOSAVESECS` (0 turns the timer off, otherwise 10 to 3600) and `BACKUPCOUNT` (0 to 50, 0 keeps none), set with
`SETVAR` for the session, or for every session in the shell preferences `%LOCALAPPDATA%\AAD\shell.json` (`"autosaveSeconds": 120`,
`"backupCount": 5`, applied when the window starts). They are session settings of the program (`aad_engine::sysvars::AUTOSAVE_SECS`,
`BACKUP_COUNT`), never written into the drawing. There is no settings dialog for them yet. Even with `BACKUPCOUNT` 0 the copy a re-import or
a restore makes of what it replaces is kept.

## Autosave (`crates/aad-host/src/autosave.rs`, `crates/engine/src/recovery.rs`)

- **It never touches the user's file.** A snapshot goes to the recovery folder only. The `.osp`, its `meta.revision`, its `change` log and its
  modified time are exactly what the last save left; MynaOps sees nothing.
- **It does not block the UI.** `Autosave::tick` runs on every pump of the host and compares pointers and clocks (the drawing is a persistent
  structure, so "has it changed" is one pointer comparison). The snapshot itself is taken of an immutable copy of the drawing on a thread
  (`aad-autosave`), and encoded on a pool of two threads so a first snapshot of a very large drawing does not take every core.
- **It is incremental.** The first snapshot writes the whole drawing; after that only the parts an edit touched are encoded
  (`aad_io::native::Encoder`) and written (a new `pack-N.bin`), then the manifest is swapped. When more than 8 packs exist, or more than half of
  the bytes in them are dead, the next snapshot writes one fresh pack and the old ones are removed, so the folder stays bounded.
- **It adapts.** The interval is at least 20 times, and the settle time at least 4 times, the duration of the last snapshot; a snapshot never
  starts while one is running. Measured on apw.dwg (232 212 entities, release): starting a snapshot costs the UI thread 0.13 ms; the first
  snapshot takes 1.86 s on the worker and writes 128.6 MB (407 parts); after a one-entity edit, 11 ms and 26 KB (1 part). The recovery folder
  of that drawing is 122.7 MB. With a 1.86 s snapshot the interval stays at the configured 120 s (20 x 1.86 s = 37 s).
- **A drawing that is merely unsaved from the start** (flattened on import) is not snapshotted until it is edited: there is nothing to recover.
- **Every write is whole or absent**: a pack and the manifest are written under a temporary name, flushed to the disk (`sync_all`) and renamed. A
  crash between pack and manifest leaves the previous manifest, which still describes a complete earlier snapshot
  (`a_crash_in_the_middle_of_a_snapshot_leaves_the_previous_one_whole`).
- **Failure is shown.** A snapshot that cannot be written (disk full, folder not writable) sets the status line to `Autosave failed: <reason>`;
  the next attempt is 30 s later.

Folder layout: `manifest.json` (format, native format, pid, time, the drawing's file, title and source, entity count, and for each part its hash,
pack, offset and length), `pack-N.bin` (parts, concatenated), `lock` (held open with no sharing by the program that owns the folder).

## Crash recovery

On start the shell asks the host for `recovery.list`: the folders under the recovery root whose `lock` nobody holds (their program is gone) and
whose manifest is readable.

- A snapshot **newer than its file** (or of a drawing that never had a file, or whose file is gone) is offered: **Restore**, **Discard** or
  **Ask me next time**.
- A snapshot not newer than its file (the file was saved after the snapshot) is removed without asking. "Newer" compares nanoseconds: the
  snapshot records the last-write time of the file (and of its SQLite `-wal`) when it was taken; a file still at that time, or written before
  the snapshot, has not been saved since.
- **Restore** (`recovery.restore {id}`) opens the drawing with unsaved changes, tied to the file it was being edited in, so **Save** writes that
  file. Every part is checked against its SHA-256 while it is read; a damaged snapshot is an error, never a partial drawing. The snapshot is kept
  until the restored drawing has been saved or has a snapshot of its own (which is taken at once).
- What a crash loses: the edits made after the last snapshot, so at most `AUTOSAVESECS` seconds of continuous editing, or 10 s after a pause.

A normal end (the window is closed, drawings saved or given up) removes the snapshots, so nothing is offered after it.

Test: `a_killed_host_leaves_a_snapshot_the_next_start_restores` (`crates/aad-host`) starts a second process (the test binary itself, running
`recovery_child_process`) that opens a DXF through its working copy, edits, takes two snapshots, edits again and waits; the test kills it, then
lists, restores and saves in a new host and checks the drawing (the edit after the last snapshot is lost, the others are back), the untouched
source, that the project file's revision did not move until the save, and that the snapshot folder is gone afterwards.

## Backups (`crates/engine/src/backup.rs`)

- **When:** `nativefile::write_project`, before the transaction that replaces the drawing, when the drawing's bytes differ from what the file
  holds, the file already had a drawing, and `BACKUPCOUNT` is not 0. A save that changes nothing makes no backup. If the backup cannot be made the
  save fails with the reason and the file is unchanged (a save never destroys the only earlier version).
- **How:** SQLite `VACUUM INTO` a temporary name in the backups folder, then a rename. It reads one consistent state, so it is safe while MynaOps
  has the file open, and a backup is a complete project file that opens by itself.
- **Pruning:** after each new backup. Only files named `<name> <16-character UTC stamp>[ reason].osp` are counted or removed; anything else in the
  folder is left alone.
- Reasons in the name: `save`, `before-reimport` (the source DWG or DXF changed and was imported again), `before-restore`.
- Cost: one copy of the project file per changing save (see PERFORMANCE.md for the time on a 250 MB file), and up to `BACKUPCOUNT` files of that
  size on disk.

### Restoring a backup

- **In the app:** File > Restore from Backup... lists the backups of the open project file, newest first. **Open a copy** opens the selected one
  as a new unnamed drawing and changes nothing. **Restore** asks about unsaved changes, replaces the project file with the backup (the file as it
  was is first kept as a `before-restore` backup), and reopens it.
- **By hand:** a backup is an ordinary `.osp`. Copy it out of `<name>.backups` and open it.
- **At the command line or from a script:** the engine command `BACKUP.RESTORE` (also the File menu item when the shell does not handle it)
  lists the backups of the active project file without arguments, and with `{backup: <path>}` restores one and reopens the drawing from it
  (refused while the drawing has unsaved changes unless `discard: true`).
- **By command (control channel or `aad_host_app`):** `backup.list {path?}`, `backup.open {backup}`, `backup.restore {backup, path?}`. A restore
  replaces the file by renaming a complete copy over it; it fails, changing nothing, while another program holds the file open. Only a file
  listed as a backup of that project file is accepted.

## Saving

- **Project files** are written in place by SQLite transactions (WAL), as before: a failed or interrupted save rolls back, the file is never
  truncated. The drawing bytes, the structural details and the native model go in one transaction.
- **DXF and DWG files** (an export, or a drawing kept as a plain DXF or DWG) are written to `<file>.aad-tmp`, flushed (`sync_all`, new) and
  renamed; the first overwrite keeps `<file>.bak` as before.
- **A new working copy** is built under a temporary name and renamed when complete ([NATIVE-OPEN.md](NATIVE-OPEN.md)).
- On OneDrive or a network folder the same steps apply; nothing is special-cased. Recovery snapshots are on the local disk whatever the drawing's
  folder is.

## What the user sees

- The tab and the window title carry ` *` while the drawing has unsaved changes.
- The status bar shows `Saved 14:32`, `Autosaved 14:35`, `Save failed: <reason>` or `Autosave failed: <reason>` (control method
  `autosave.status`). "Saved" is noticed from the drawing's saved state, so a save from the menu, the command line or a script all count.
- A failed save is never silent: the reason is the error of the save, a line in the command history (`Save failed: ...`) and the status bar
  (for a save typed at the command line the reason is in the command history; the status bar shows failures of the File menu, `app.save`
  and the `save`/`saveas` engine commands).

## Control methods

`autosave.status`, `autosave.now {wait?}` (snapshot every drawing with unsaved changes now), `recovery.list`, `recovery.restore {id}`,
`recovery.discard {id}`, `backup.list {path?}`, `backup.open {backup}`, `backup.restore {backup, path?}`, `app.peek {path}`.

## Measured (apw.dwg, 232 212 entities, release, 2026-11-11)

| | |
|---|---|
| Backup of the 239 MB working copy (`VACUUM INTO` and rename) | 619 ms |
| Save into the working copy (backup, DWG bytes, native model, one transaction; on the save worker) | 9.7 s |
| Snapshot: UI thread / first (worker) / after a one-entity edit (worker) | 0.13 ms / 1.86 s / 11 ms |

On Working Drawing.dwg (26 210 entities): backup 27 ms, save 1.27 s, snapshots 46 ms then 8 ms.

## Limits

- Snapshots are per program run and local to the machine; they are not synced.
- The undo history is not in a snapshot: a restored drawing starts with an empty undo list.
- A drawing kept as a plain DXF or DWG (opened with the engine `open` command, or exported) has snapshots but no rolling backups, only the
  single `.bak` and the drawing history that existed before.
- Backups cost disk space in proportion to the project file; lower `BACKUPCOUNT` for very large drawings.

## Not verified

- The WinUI shell code (`apps/aad-winui/SafeFiles.cs`) compiles but was not run in a window: the restore question on start, the File >
  Restore from Backup dialog, the `SaveState` status text and the ` *` in the title were not seen on screen. The host methods they call are
  covered by the host tests (`the_status_line_reports_saves_autosaves_and_failures`, `backups_are_listed_and_restored_through_the_app_methods`,
  `a_killed_host_leaves_a_snapshot_the_next_start_restores`).
- Saving on OneDrive or a network folder was not tested.
