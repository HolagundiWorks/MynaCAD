# Opening a DWG or DXF: the native working copy

Status: **implemented** in the engine, the host (`app.open`, the WinUI open path) and `aad-cli`, with tests, and measured on the two drawings
below. The shell part (status text, the "source changed" question) is implemented in `apps/aad-winui` and compiles; it was not driven in a
running window (see "Not verified" at the end).

## What happens

MynaCAD edits one format: the project file (`.osp`, format `osp` version 1, SQLite; see [PROJECT-FILE.md](PROJECT-FILE.md)). A drawing that is not a
project file (a `.dwg` or a `.dxf`) is **converted once** into a project file, its *working copy*, and edited there.

- The original is only ever read. **File > Save** writes the working copy. A DWG or DXF is written only by **Save As / Export** with a `.dwg` or
  `.dxf` name, and that is a copy: the drawing stays in its project file, with its unsaved changes (the command line says "Exported a copy to ...").
- The next time the same DWG or DXF is opened, the working copy is opened instead, without parsing the DWG again, for as long as the original has
  not changed.
- The tab shows the working copy's name (`plan.osp`) and where it came from; the status bar says where the working copy is.

Code: `crates/engine/src/nativefile.rs` (the rules below), `crates/io/src/native.rs` (the encoding), `crates/odp/src/project.rs` (the tables),
`crates/aad-host/src/lib.rs` (`open_start_with`, `open_poll`, `open_progress`, `open_info`).

## Where the working copy lives

One rule, in this order (`nativefile::slot_for`):

1. `<folder of the source>/<name>.osp`, when that file **is already this source's working copy** (its `meta` says `source.name` = the source's file
   name).
2. The same path, when **no file of that name exists** and the folder can be written.
3. Otherwise (a project file of the user's own, or any other file, already has that name; or the folder is read-only): the per-user cache,
   `%LOCALAPPDATA%\OSAS\native-cache\<16 hex digits of SHA-256 of the source's full path, lower case>\<name>.osp`. `AAD_NATIVE_CACHE` moves the cache.

An existing `.osp` that is not this source's working copy is never opened for writing by an import and never replaced. The conversion is built under
`<name>.osp.importing-<pid>` and renamed into place when it is complete and flushed, so a failed or interrupted conversion leaves no `.osp`.

The status line after the open says which case applied, for example: `apw.dwg was converted to the working copy D:\jobs\apw.osp. Save writes the
working copy; the original is not changed.`, with `(in the MynaCAD cache folder)` added in case 3.

## What a working copy holds

| Where | What |
|---|---|
| table `drawing` | The **source bytes, untouched**, under the source's file name (`apw.dwg`), until the first save. A save replaces them with the drawing written by MynaCAD in the same format (DWG stays DWG, DXF stays DXF), exactly as saving into any `.osp` always did. MynaOps and older MynaCAD versions read this row; nothing about it changed. |
| tables `native_part`, `native_blob` (new) | The **native model**: the drawing as MynaCAD holds it in memory, in parts. `native_part(drawing, seq, hash, entities)` is the ordered list, `native_blob(hash, bytes)` holds each distinct part once, by SHA-256. |
| `meta` keys `native.format`, `native.drawing`, `native.drawing_sha256`, `native.saved` (new) | The encoding version, which drawing the model belongs to, and **the SHA-256 of the `drawing` row it was made from**. |
| `meta` keys `source.path`, `source.name`, `source.size`, `source.mtime_ns`, `source.sha256`, `source.drawing_sha256`, `source.import_version`, `source.imported`, `source.insunits` (new) | Where the working copy came from: the source's path, size, last-write time (nanoseconds since 1970), SHA-256 of the whole file, the reader version (`nativefile::IMPORT_VERSION`), the time of the import, and the drawing's `INSUNITS` as read. |
| `meta` keys `origin`, `drafting_units`, `drafting_unit` | Unchanged conventions of the geometry database (`crates/geodb/src/origin.rs`, `write_project_meta`): written at import only when the drawing carries them, and on every save as before. A drawing from another program is **not** stamped at import, so it is still detected as foreign; the stamp value written on save is still `"app":"Tren"`. |

Compatibility: no existing table or key changed. The new tables are created with `CREATE TABLE IF NOT EXISTS` when a file is opened, like the other
tables. The native model and the new keys are local data: they are not in the `change` log and do not raise `meta.revision` (`Edit::put_native`,
`Edit::set_local_meta`), so sync and the MynaOps live reload see exactly what they saw before. The import itself is one revision (the `drawing` row).
The .NET-written fixture (`crates/odp/tests/from_dotnet.rs`) and the two-writers test still pass.

### The native model is a cache of the `drawing` row, and says so

`nativefile::read_project` uses the native model only when `native.format` is the version this program reads, `native.drawing` is the file's first
drawing and `native.drawing_sha256` equals the `sha256` column of that `drawing` row (one indexed read, no bytes). If MynaOps, an older MynaCAD or
anything else replaces the drawing, the hashes differ and the drawing is parsed from the row, as before; the next save from MynaCAD writes the model
again. A model that fails to decode (damaged, truncated) is also a fallback to the row, never a partial drawing.

### The encoding (`aad_io::native`, format 1)

Part 0 is the head: header variables, layers, linetypes, styles, block and layout records, levels, sheet sets, views, groups, project data, the
handle seed. Every later part is a batch of entities for named stores (`model`, `block:<n>`, `layout:<n>`, `level:<id>`, `sheet:<set>:<level>`).
Model space is cut at the entity store's own chunk boundaries (1024 entities), and a chunk is cut again when its entities weigh more than about 1 MB,
so an embedded OLE object (megabytes of text in a DWG) is a part of its own. Each part is MessagePack **with field names** (a field added later with
a default still reads old files), deflated (level 1), behind a one-byte codec tag. Parts are built and read on every core. `native::Encoder` keeps
the parts of the chunks an edit did not touch, so a later save or autosave encodes only what changed.

Reading checks sizes before allocating (a part inflates to at most 1 GiB, at most 4 000 000 parts) and every error is a `Result`: tests feed
truncated, swapped and bit-flipped parts and a deflate bomb (`damaged_or_hostile_parts_are_errors_not_panics`).

What is stored is the drawing **as the reader produced it**, before the import's flattening to 2D, so opening the working copy runs the same
`prepare` step as a direct read and gives the same "Flatten on import" undo step and messages.

## When the source has changed

On every open of a DWG or DXF the source is read and hashed (SHA-256 of the whole file: 31 ms to read and 55 ms to hash the 115.7 MB test
drawing, measured) and compared with `source.size` and `source.sha256`. A different last-write time alone is not a change.

| Source | Working copy | What happens |
|---|---|---|
| unchanged | any | The working copy is opened (`action: reused`). |
| changed | never saved since its import (`drawing` row still has `source.drawing_sha256`) | It is converted again by itself (`reimported`): there is nothing of the user's to lose. |
| changed | saved since its import | The shell asks: **Import again** or **Open the working copy**. Without a person (control channel, `aad-cli import`, `onChanged: auto`) the working copy is opened as it is (`kept`, `sourceChanged: true`) and the status line says so. |
| unchanged, but imported by an older reader (`source.import_version` differs) | not saved since | Converted again. A saved one is kept. |

A re-import never deletes: the whole project file as it was is first copied to `<name>.backups\<name> <UTC time> before-reimport.osp` (see
[AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md)), then the drawing and the native model are replaced **inside the same file** in one transaction, so the
project keeps its identity (`project_id`), take-off, project record, markups and history.

`nativefile::peek(path)` answers "would this open ask?" without converting (state `new`, `current`, `changed`, and `edited`); the shell calls it
(control method `app.peek`) before it opens.

## Progress and failure

The conversion runs on the existing open worker (`aad-open` thread); the UI thread only polls. `Host::open_progress()` (FFI `aad_host_open_progress`)
gives the stage for the busy indicator: "Checking apw.dwg", "Converting apw.dwg to the native format (first open of this version of the file)",
"Writing the working copy apw.osp", "Opening the working copy apw.osp", "Preparing the drawing". A failure (unreadable DWG, disk full, no folder to
write to) is the open's error text; the drawing that was open stays, the source is untouched and no partial `.osp` is left
(`a_failed_conversion_leaves_no_file_and_the_source_untouched`).

`Host::open_info()` (FFI `aad_host_open_info`, and the result of control `app.open`) is JSON: `native`, `source`, `action`
(`project | imported | reused | reimported | kept`), `location` (`beside | cache`), `sourceChanged`, `edited`, `backup`, `fromNative`, `ms` (time of
each stage) and `message`.

## Command line

`aad-cli` semantics are unchanged: `run FILE`, `info FILE`, `convert IN OUT` read a DWG or DXF directly and write nothing beside it. New:

- `aad-cli convert IN.(dwg|dxf) OUT.osp`: the conversion to a named project file (OUT must not exist), printing the time of each stage.
- `aad-cli import FILE.(dwg|dxf) [--on-changed auto|reimport|keep]`: what the app does on open (make or reuse the working copy), printing the JSON
  above plus `entityCount`.
- `info`, `run` and `convert` on an `.osp` read the native model when the file has a valid one.

The engine command `open {path}` (scripts, MCP, `engine.execute`) still opens a DWG or DXF directly and ties the document to that file, as before;
only the application's open (`app.open`, File > Open, drag and drop, the recent list) goes through the working copy.

## Fidelity

Measured on Working Drawing.dwg (release, `a_real_drawing_is_the_same_through_its_native_copy`): 26 210 entities both ways, the decoded drawing
equals the direct read (`==` on the whole model), 710 250 primitives, 1 137 662 line vertices and 14 961 024 triangle vertices both ways, and 0
of 3 840 000 PNG pixels differ by more than 2 grey levels. `aad-cli info` took 1030 ms on the DWG and 284 ms on the `.osp`.
On apw.dwg: 232 212 entities, 3 488 186 primitives, 6 112 531 line vertices and 29 980 602 triangle vertices both ways, 0 of 3 840 000 pixels
different; `aad-cli info` 6502 ms on the DWG, 2052 ms on the `.osp`.

- `aad_io::native` unit tests: a drawing with model space, blocks, layouts, levels and sheet sets decodes to an equal `Drawing` (`==` on the whole
  model); NaN and infinity survive; the same drawing gives the same bytes.
- `crates/engine/tests/native_open.rs`: the converted drawing equals the direct read, and the reopened working copy equals it again.
- `apps/aad-cli/tests/native_fidelity.rs`: same `entityCount` from `aad-cli info`, equal drawing, same display-list counts (primitives, line
  vertices, triangle vertices at full detail), and the PNG of the direct read against the PNG through the `.osp` (pixels differing by more than 2
  grey levels must be under 0.01 %). Runs on the sample drawing in CI and on a real drawing with
  `AAD_PERF_FILE=<file> cargo test --release -p aad-cli --test native_fidelity -- --ignored --nocapture`.

## Measured

Release build, headless host (`native_open_timing` in `crates/aad-host/src/lib.rs`), 2026-11-11. "Open" is `app.open` (read, convert or decode,
prepare, make the document, fit); "first frame" is the first display list and mesh (`Host::mesh`, no GPU). One run each; the machine was also
running other builds, so treat them as indications.

| | Working Drawing.dwg (3.5 MB, 26 210 entities) | apw.dwg (115.7 MB, 232 212 entities) |
|---|---|---|
| Direct read of the DWG (before this change, every open) | 1383 + 192 = 1575 ms | 6117 + 721 = 6837 ms |
| First open: convert to the working copy | 1033 + 110 = 1143 ms (convert 831, encode 17, write 29) | 7438 + 619 = 8057 ms (convert 5520, encode 381, write 622) |
| Later opens: the working copy | 220 + 113 = **333 ms** (hash 3, decode 58) | 1882 + 668 = **2550 ms** (hash 87, decode 761) |
| Working copy size | 5.7 MB | 239.1 MB (115.7 MB source bytes + about 123 MB native model) |
| After the first save | 8.7 MB | 258.8 MB |

Writing the 232k-entity working copy takes about 1.0 s (encode 381 ms on every core, SQLite write 622 ms, most of it the 115.7 MB of source
bytes and 123 MB of parts); the first open is dominated by the DWG conversion itself (5.5 s), which it does once. The remaining warm cost is the
decode (0.76 s) and `prepare` (flatten to 2D and the copy kept as the saved state, about 1 s). More in [PERFORMANCE.md](PERFORMANCE.md),
section "Native working copy, autosave and backups (2026-11-11)".

## Limits and things not done

- The working copy stores the source bytes as well as the native model, so it is larger than the source (about twice for a DWG dominated by
  embedded OLE objects). This is the price of keeping the `drawing` row readable by MynaOps without a save.
- A save still writes the `drawing` row in DXF or DWG besides the native model (so other programs see the saved drawing). For a very large drawing
  that is the slow part of a save; it runs on the save worker. See PERFORMANCE.md for the time.
- A read-only source folder is handled by rule 3 but has no automated test (the test would need to change folder permissions on the build machine).
- Working copies in the cache are never deleted automatically.
- The engine command `open` does not use working copies (see above).
- A source that changed while its working copy has no saved edits is converted again without asking (nothing of the user's is lost; the
  earlier copy is kept as a `before-reimport` backup). The question is asked only when the working copy has saved edits.

## Not verified

- The WinUI shell code (`apps/aad-winui/SafeFiles.cs`, the changes in `MainWindow.xaml.cs`) compiles (`dotnet build -c Debug -p:Platform=x64`)
  but was not run in a window: the busy text during conversion, the "changed outside MynaCAD" dialog, the tab text `plan.osp (from plan.dwg)`
  and the status message were not seen on screen. The host methods they call are covered by the host tests.
- A read-only source folder (rule 3) and the per-user cache path are not covered by an automated test.
