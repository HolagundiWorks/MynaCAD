# The project file (.osp) in MynaCAD

A project file is one SQLite file with the drawing, the take-off and the project record (format `osas-project` v1). MynaCAD and MynaOps open the same file. **Migration from ShilpiDB / `.vdb`:** [MIGRATION-OSP.md](MIGRATION-OSP.md). Suite-wide names, roles and links: [SUITE.md](SUITE.md). Format and sync plan: MynaHub repository `docs/ODP-PROJECT-FILE-AND-SYNC.md`.

| Part | Where |
|---|---|
| Format library | `crates/odp` (crate `aad-odp`): create/open, edit + commit (+1 `meta.revision`), reads, `replace_takeoff` (writes only differences), examples `pack`, `inspect`, `stress`; tests include a file written by the .NET side and four concurrent writers |
| Open / save | `crates/engine/src/cmd/file.rs`: `ContainerHooks` (read/write), `format_hint` (a new project file takes `dxf` unless told). The host writes the drawing back in place only when its bytes changed |
| Take-off | `crates/engine/src/cmd/takeoffrec.rs`: `takeoff.record` writes into the open `.osp` (`OdpSink`); with no project open it asks the user to save as one. `path` may name another `.osp` |
| UI | File > Open / Save As offer `.osp` ("Project (drawing and take-off)"); the Take-off toggle in the bottom bar shows the take-off tools |
| Structural details | Table `detail` (`crates/odp`: `Detail`, `Odp::details`, `Edit::replace_details`): one row per RCC block in the drawing (`rcc.beam.section`, `rcc.column.*`, `rcc.slab.section`, `rcc.footing.section`, `rcc.lintel.section`) with its parameters as JSON (`id` = `<drawing>/<handle>`, `kind` = the MynaOps page: beams, columns, slabs, footings, lintels). The host writes them with the drawing on every save into a `.osp`, in the same revision, and not at all when nothing changed (`aad_engine::structural_details`). MynaOps reads them and draws each read only (`OdpFile.Details()`); a drawing is changed in MynaCAD only. See [smart-blocks.md](smart-blocks.md) |
| Pointer | `apps/aad-winui/ActiveProjectPointer.cs` writes `%LOCALAPPDATA%\OSAS\active-project.json` with `project` = the open `.osp` |

ShilpiDB is gone from MynaCAD: the `library.*` and `vdb.*` commands, the `.vdb` sidecar, the status chip, the agent's ShilpiDB tools and the `shilpid` bundling were removed. The drawing history now lives in the project file (`history_record` table; `<drawing>.history.osp` beside a drawing that is not one) and does not count as a revision.

## Project configuration (`meta.config.*`)

MynaOps is the only writer of the project configuration; MynaCAD reads it and applies it to the drawing ([ops-first-workflow.md](ops-first-workflow.md), sections 4 and 7a). It is additive: new `meta` keys with the prefix `config.`; no table is added or changed, `meta.format` and `meta.version` stay as they are. Every value is JSON text of at most 256 KiB and carries `"v": 1`; a key with an unknown `v` is treated as absent and reported, unknown fields are ignored.

| Key | Written by | Meaning |
|---|---|---|
| `config.schema` | MynaOps | `{v, min_reader}`: present = the project is configured by MynaOps; a reader older than `min_reader` refuses with "update the program" |
| `config.rev` | MynaOps | `{v, rev, app, time}`: rises by one per configuration change |
| `config.project`, `config.team`, `config.units`, `config.levels`, `config.layer_sets`, `config.tabs`, `config.sheets`, `config.naming`, `config.titleblock` | MynaOps | The parts of the configuration (fields in ops-first-workflow.md 4.2; `config.tabs` also has an optional `next_tid`) |
| `config.applied.cad` | MynaCAD, **local** (no revision) | `{v, rev, time, result: ok\|partial\|failed, has_content, items: [{key, status, detail}]}`: what MynaCAD applied; `rev` is the configuration revision it tried, also for `failed` |
| `config.proposal` | MynaCAD, **local**, once | What a legacy drawing already has (levels with their `LevelId`, tabs, layer sets present, unit, details, numbering scheme), for MynaOps' Adopt page. Written only when the file has no `config.schema` |

`config.plot` is **not defined**: the inventory of project-wide plot settings found none (ops-first-workflow.md, 3.1).

| Part | Where |
|---|---|
| Library | `crates/odp/src/config.rs` (types, parse, validate; builds for the web) and `config_io.rs` (`read`, the read-only `probe`, `write`, `write_applied`, `write_proposal`; not for the web). `Odp::meta_len` (a value's size without reading it) and `Edit::put_meta` (a shared meta value: a revision and a `change` row) are the two additions to `Odp` |
| One revision per write | `config::write` puts every changed part and the bumped `config.rev` in one `Edit`, so `meta.revision` rises once; nothing is written when no part differs. The Rust writer exists for tests and tools; MynaOps writes with `Office.Shared.ProjectConfig.Write`, which also sets `sync.last` |
| Applying | Engine command `project.config.apply` (`crates/engine/src/cmd/projconfig.rs`): one undo entry "Project configuration", whole or nothing, a no-op writes nothing. `project.config.propose` builds the proposal. The ids are bound in the drawing's project map: `config.levelmap` (`cid` to `LevelId`), `config.tabmap` (`tid` to sheet set id); a take-off header `levels` entry gets `config_id` (the `cid`) next to `id` when the level is bound |
| Fixtures | `crates/odp/tests/fixtures/config`: the same files are committed in MynaOps (`BBSDesktop/Office.Shared.Tests/Fixtures/config`); `config-rust.osp` was written by the Rust writer and `config-csharp.osp` by the C# writer, each language reads the other's |
