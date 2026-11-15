# The Ops-first workflow: MynaOps opens the project, MynaCAD is launched from it

Status: **design, decided by the owner on 2026-11-15, with every open question of section 8 decided the same day (accept the recommendation); nothing in this document is implemented.** It is a contract and a staged plan. "Today" below
describes what the code does now (each claim names where it was read); "Planned" describes the target. Canonical copy: MynaOps `docs/ops-first-workflow.md`;
MynaCAD carries an identical copy. Suite-wide decision record: `docs/SUITE.md` (section 0). Written without changing any code.

Related, read together: live-sync.md (MynaCAD branch `claude/osp-live-sync-aa35c6`, MynaOps branch `live-sync`; change detection and a status text between the two
running apps; not merged when this was written), [NATIVE-OPEN.md](NATIVE-OPEN.md), [AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md),
[levels-and-sheet-sets.md](levels-and-sheet-sets.md), [PROJECT_WIZARD.md](PROJECT_WIZARD.md), [PROJECT-FILE.md](PROJECT-FILE.md).

## 0. The owner's decision (verbatim, typos cleaned)

> For CAD and OPS, the optimised workflow will be: launch the Ops app first, select the file to be opened or create a new project, enter required project detail and
> create project; AI start/stop stays in Ops, also DB server start/stop, and CAD gets launched from here with the current project. There is no option for CAD to open a
> project file individually; it will be through the Ops app only. Move the project wizard, settings and project-related configurations (levels, layer sets) to the Ops
> project wizard; anything like this needs to be configured in Ops and gets updated in CAD. CAD will have only import and export functions, and a single file will be
> running across both apps, so no confusion and this clears our work flow. Add realtime sync on/off and sync now buttons on both the apps' tool sets.

### Decisions this design embodies

| # | Decision | Why / consequence |
|---|---|---|
| D1 | **MynaOps is the front door.** A project is created or chosen in MynaOps; MynaCAD is started by MynaOps with that project | One entry point; removes "which file is open where" |
| D2 | **Exactly one `.osp` is live across both apps**, enforced by a lease (section 2.3), not by convention | MynaCAD cannot be pointed at a different file than MynaOps shows |
| D3 | **Project configuration is owned and written by MynaOps only** (details, team, units, levels, layer sets, discipline tabs, sheet numbering, naming, title-block data, project-wide plot defaults). MynaCAD reads it and applies it to the drawing | Single writer per datum; no merge, no conflict |
| D4 | **Drawing content is owned and written by MynaCAD**; the take-off is written by MynaCAD; quantities, estimate, bills by MynaOps (unchanged from live-sync "who owns what") | Unchanged |
| D5 | **MynaCAD's File menu is import and export only** (plus Save into the project, Print/Plot, History): no Open, New, Save As of project files, no Project Wizard, no project settings dialog, no level or layer-set configuration UI | The owner's words; "Save" stays because the project file is the drawing's store |
| D6 | **AI start/stop and the project-database control live in MynaOps** and MynaCAD never starts MynaOps' headless AI host in managed mode | Ops is always running when CAD runs |
| D7 | **Sync on/off and Sync now** are in the tool sets of both apps, and build on the live-sync detector (section 2.6) | "Off" pauses incoming refresh and apply, never writes |
| D8 | **The configuration is additive `meta.config.*` keys** in the `.osp` (section 4); `osp` stays version 1 | MynaOps and MynaCAD share the file; additive-only rule from SUITE.md |

## 1. User flow, step by step

### 1.1 Planned flow

1. **Start MynaOps** (Start menu; or double-click an `.osp`, which opens MynaOps, section 5). No project open: the Project home shows **Open project...**, **New project...**, and the recent list.
2. **Open project** (file picker for `.osp`; also `.bbsproj` / `.json` as today) or **New project**.
3. **New project wizard** (in MynaOps; fields in 1.2). **Create project** creates the `.osp` (MynaOps' `OdpFile.Create(path, name)` exists today), writes the configuration (`meta.config.*`), and opens it.
4. **Project home** (planned page; today the nearest pages are Project > Dashboard, Levels, Take-off, Project database): the project name and details, the levels, the layer sets and tabs, the file's path and size, **Database** state (1.3), **AI** state with **Start / Stop**, **Sync** state with **Realtime on/off** and **Sync now**, and one primary button **Open MynaCAD**.
5. **Open MynaCAD** starts MynaCAD with this project (contract in 2.1). MynaCAD opens straight into the project: the configured levels exist as level tabs, the layer sets and discipline tabs are set, units and project details are applied. If MynaCAD is already running for this project, **Open MynaCAD** brings it to the front.
6. **Work.** Drafting, take-off and quantities as today. Changing the project configuration is done in MynaOps and appears in MynaCAD without reopening (section 4.4). MynaCAD's status bar shows the sync text; MynaOps' status strip shows the same text (live-sync).
7. **Close.** Closing MynaCAD saves or asks (as the close question does today) and releases its lease; MynaOps stays open. Closing MynaOps while MynaCAD is open asks: **Close both** (MynaCAD is asked to save and close first) or **Keep MynaCAD open** (section 6, "Ops closed while CAD is open").

### 1.2 Create-project wizard fields (Ops)

Taken from what MynaCAD's Project Wizard has today (`apps/aad-winui/ProjectWizardWindow.cs`, engine `project.wizard`, `crates/engine/src/cmd/wizard.rs`, [PROJECT_WIZARD.md](PROJECT_WIZARD.md)) plus what MynaOps' Settings page already holds (project name, persona):

| Page | Fields |
|---|---|
| Project | File name and folder (location of the `.osp`); name, client/owner, location, site address, property ID, site area, plot use, authority, date, summary; **persona** (Project Manager/PMC or Contractor; Ops already has it) |
| Team and company | Architect, engineer; **Read the company file** (MynaHub company details, as the MynaCAD wizard button does today; MynaOps does not open the `.osas` today, so this is the existing exported pack, `company.pack.import`, not a new link to MynaHub) |
| Building | Drawing unit; floors above ground, basements, terrace; default floor-to-floor height; editable list of levels (name, elevation, height); slab and beam defaults; "one level per floor" |
| Standards | Layer sets (`standard`, `permit`, `presentation`), the five discipline tabs and symbol blocks, sheet numbering scheme |
| Review | Everything above; **Create project** |

Not in MynaCAD's wizard either (stays unbuilt): plot scale, per-floor elevations other than the running sum, templates and saved drafts, logo, several architects.

### 1.3 "DB server start/stop"

Today there is **no database server**: the project database is one SQLite file (WAL) and MynaOps' manager (Project > Project database) has only maintenance commands (info, check integrity,
compact, back up now, restore) (MynaOps `docs/PROJECT-FILE.md`; `Office.Shared/ProjectDatabase.cs`). This design reads "DB server start/stop" as **Database online / offline** in MynaOps:
*online* = MynaOps holds the project open and its lease; *offline* = MynaCAD is closed (MynaOps asks it), the write-ahead log is folded in (checkpoint), the leases are released, and compact, restore or copy-the-file are allowed. Decided as Q1 (section 8).

### 1.4 What each app shows when the other is absent

| Situation | MynaOps | MynaCAD |
|---|---|---|
| MynaCAD not running | Project home: **MynaCAD: not running** and **Open MynaCAD**; Take-off page shows the last recorded take-off (as today), sync text `Synced` or `Updated by MynaCAD at hh:mm` | n/a |
| MynaOps not running, MynaCAD started directly | n/a | Launch screen: "MynaCAD opens projects from MynaOps" with **Open MynaOps** (2.4). No drawing, no File > Open |
| MynaOps closed while MynaCAD is open (user chose Keep open, or MynaOps crashed) | n/a | Keeps working on the project: drawing, save, take-off record (it holds the file). Status bar: `MynaOps closed`; configuration changes cannot arrive; the AI agent is unavailable unless a headless host is running (section 5); **Open MynaOps** button re-attaches |
| AI stopped in MynaOps | Status strip `AI: off`; **Start** | The orb tooltip says `stopped in MynaOps`; MynaCAD does not start it |

## 2. Process model

### 2.1 Launch contract: MynaOps starts MynaCAD

Today: MynaCAD takes `--sample`, `--control PORT` (or env `AAD_CONTROL_PORT`), `--` flags it ignores, and any other argument as a file to open (`apps/aad-winui/LaunchArgs.cs`).
MynaOps has no code that starts MynaCAD that was found in this review (not checked beyond `Program.cs`, `MainWindow.Ai.cs` and a search of `BBSApp`).

Planned command line (`ODraft.WinUi.exe`, the identifier keeps its name):

```
ODraft.WinUi.exe --managed --project "<absolute path of the .osp>" --ops-pid <MynaOps process id>
                 [--import "<file.dwg|.dxf>"] [--control <port>]
```

| Argument | Meaning |
|---|---|
| `--managed` | Managed mode (2.4): no File > Open/New/Save As of project files, no wizard, configuration read-only. Without `--managed` and without `--standalone` MynaCAD shows the launch screen |
| `--project` | The project file. Required with `--managed`. MynaCAD refuses a path that is not a project file of format `osp` (a project, never a `.dwg`/`.dxf`) |
| `--ops-pid` | Lets MynaCAD show `MynaOps closed` when that process ends and re-attach to a new MynaOps |
| `--import` | Import this file into the project after opening (dialog 3.4); how a `.dwg` handed to MynaCAD by double-click reaches the project (2.4) |
| `--standalone` | **Not a user feature.** Today's behaviour, kept for `aad-cli`, the control channel tests, MCP and developers (section 5); not shown in help or installer shortcuts |

No secrets are passed on the command line or in the environment (SUITE.md: MynaHub passes nothing either).

**One MynaCAD per project.** MynaCAD takes a named mutex `Local\MynaCAD.Project.<first 16 hex digits of SHA-256 of the lower-case full path>`. A second start for the same project does not open a
window: it connects to the running instance's **activation pipe** `\\.\pipe\MynaCAD.<same hash>` (current-user only) and sends `activate`, or `import <path>`, then exits. The pipe carries JSON lines for the suite-internal requests only: `activate`, `import {path}`,
`close {save: "ask"|"save"|"discard"}`, `sync.set {on}`, `sync.now`, `status`. MynaCAD's existing loopback control channel (`--control`) stays a developer and test channel and is not used between the two apps.

MynaOps writes nothing about a MynaCAD it started except what the lease and status already show (2.3). MynaOps does not read MynaCAD's drawing.

### 2.2 Who owns which tables and meta keys

Extends the live-sync table (live-sync.md, "Who owns what"). A program writes only what it owns; the rule from live-sync stays: a program that would have to write a table it does not own refuses.

| Data | Writer | Reader |
|---|---|---|
| `meta.config.*` (section 4), `meta.name` | **MynaOps** | MynaCAD (read only) |
| `project_doc` (estimate, rates, bills, project record) | MynaOps | MynaCAD does not read it |
| `drawing`, `drawing_state`, native model (`native_part`, `native_blob`, `native.*`), `detail`, `history*`, `source.*` | **MynaCAD** | MynaOps (drawing, details) |
| `takeoff_header`, `takeoff_row`, annotated PDF assets, `sync.takeoff.*` | MynaCAD (`takeoff.record`) | MynaOps |
| `meta.config.applied.cad` (what MynaCAD has applied: revision, time, per-item result), `meta.config.proposal` (section 4.5, one-time adoption proposal) | MynaCAD, **local keys: no revision** (like `native.*`, `Edit::set_local_meta`) | MynaOps |
| `sync.last`, `meta.revision` | whoever commits | both |
| `meta.session.holder` (advisory, 2.3) | MynaOps | both |

`config.applied.cad` and `config.proposal` are the only two config-prefixed keys MynaCAD writes; they are reports, not configuration, so the "Ops writes config" rule holds.

### 2.3 One live file: the lease

Goal (D2): at most one MynaOps and one MynaCAD have the project, always the same file, and nothing else opens it for writing behind them.

| Piece | Mechanism | Verified |
|---|---|---|
| Same-PC lease | Each app, when it opens the project, creates `%LOCALAPPDATA%\O-Studio\leases\<path hash>.ops.lock` / `.cad.lock` and **keeps it open with no sharing** (like the recovery folder's `lock`, [AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md)); the lock disappears with the process, so a crash never leaves a stale lease. A small JSON body (`{app, pid, started_utc, project}`) is written for display. The path hash is the same as the mutex name (2.1) | Design. The recovery-folder `lock` pattern is documented as implemented |
| Replaces | MynaOps' "is MynaCAD running" test today: the active-project pointer plus a process named `ODraft.WinUi`, or an exclusive-open probe (MynaOps `docs/PROJECT-FILE.md`, "Open elsewhere"). The pointer `%LOCALAPPDATA%\O-Studio\active-project.json` (`ActiveProjectPointer.cs`, written on every active-drawing change) stays written for compatibility until migration (stage E) and is then removed | Read |
| Maintenance gate | Compact and Restore require that **no `.cad.lock` is held** (today: "refused while the file is open elsewhere") | Existing rule, new test |
| Other machine | A lease file is per user profile, so it cannot protect a file on a network share. Advisory only: MynaOps writes the **local** meta key `session.holder` `{machine, user, ops_pid, started_utc, heartbeat_utc}` on open, refreshes it every 30 s, clears it on close; a MynaOps on another PC that sees a heartbeat younger than 120 s warns and offers **Open read-only** or **Take over** | Design; see risk R4 |

MynaCAD never chooses a file: it opens the one named by `--project`, and while running it will not change to another. If MynaOps switches projects (File > Open another), it asks to close MynaCAD for the old one first, or starts a second MynaCAD for the new project (allowed: one per project; see Q6).

### 2.4 MynaCAD started without MynaOps

| How MynaCAD is started | What happens |
|---|---|
| Double-click `ODraft.WinUi.exe`, Start menu shortcut, or a Hub status-row start (no arguments) | Launch screen, no drawing: **Open MynaOps** (starts `O-CoreOp.exe`; if MynaOps is already running it is brought to the front) and, in MynaOps, the user picks or creates a project and presses **Open MynaCAD**. MynaCAD exits after handing over (the second MynaCAD that MynaOps starts is the real one) |
| With a `.dwg` or `.dxf` argument (Explorer "Open with", drag onto the exe) | MynaCAD starts MynaOps with `--import-pick "<file>"`; MynaOps asks **Import into the current project / New project from this file / Cancel** and then starts MynaCAD with `--project ... --import "<file>"` (3.4). A `.dwg`/`.dxf` is never opened as a working copy beside the source any more |
| With an `.osp` argument | The `.osp` double-click verb becomes MynaOps (section 5). A MynaCAD started with an `.osp` argument and no `--managed` forwards it to MynaOps the same way as a `.dwg` |
| While MynaCAD is already running for a project, a second start with a file | The activation pipe forwards `import <path>` to the running instance; no second window |
| `--standalone` | Today's behaviour (File > Open etc.), for tests and developers only |

Whether MynaCAD may run alone as a scratchpad was Q2 (decided: no scratchpad).

### 2.5 Autosave, backup, recovery and the database manager under the new model

| Piece | Today | Under Ops-first |
|---|---|---|
| Autosave snapshots | MynaCAD, recovery folder only, never the `.osp`; `AUTOSAVESECS` 120 s ([AUTOSAVE-BACKUP.md](AUTOSAVE-BACKUP.md)) | Unchanged. The snapshot records which project file the drawing belongs to; on restore after a crash MynaCAD is started by MynaOps with `--project` and offers the restore as today (the restore question is asked by MynaCAD, not MynaOps) |
| Backups (`<name>.backups`, `BACKUPCOUNT` 5) | MynaCAD before every changing save (`save`, `before-reimport`, `before-restore`); MynaOps manager adds `manual` | Unchanged. MynaOps' **Back up now** stays safe while MynaCAD is open (`VACUUM INTO`) |
| Restore | MynaCAD File > Restore from Backup; MynaOps manager Restore (refused while open elsewhere) | **Project-level restore moves to MynaOps** (the manager), which closes MynaCAD first (`close {save:"ask"}` through the pipe), restores, reopens. MynaCAD keeps **Open a copy** of a backup only if Q5 says so; its own Restore menu item goes (it replaces the project file the other app has open) |
| Drawing history / milestones (`history_record`) | MynaCAD, History window | Unchanged (a drawing feature) |
| Integrity, compact | MynaOps manager | Unchanged; refused while the CAD lease is held |
| Native working copy | A `.dwg`/`.dxf` opened beside itself as `<name>.osp` | **Replaced by import into the project** (3.4); `nativefile` stays as the import engine and its tests |

### 2.6 Relation to live sync, and the Sync buttons

Live-sync (design, MynaCAD branch `claude/osp-live-sync-aa35c6`, MynaOps `live-sync`) already defines: `meta.revision` poll as the signal (MynaOps every 2 s, with a watcher hint; MynaCAD `project.sync` about every 3 s), keys `sync.last` and `sync.takeoff.<slot>`, and the status texts `Synced`, `Updated by …`, `Take-off out of date`, `Locked`, `Project file was replaced`, `Project file unreadable`, `Project file missing`. Its rule 2, **never reload under the user**, holds here too: incoming changes either are applied through a defined, safe path (configuration, 4.4) or only announced.

| Control | Meaning | MynaCAD | MynaOps |
|---|---|---|---|
| **Realtime sync: on** (default) | Incoming change detection and refresh are active | The 3 s probe runs; a newer `config.rev` is applied at once when idle (4.4); other changes (take-off written by someone else, replaced file) are announced | The 2 s poll and watcher run; the Take-off page and levels refresh; the status text updates |
| **Realtime sync: off** | Incoming detection and refresh **paused**; **writing is never paused** (a save or take-off record still writes) | No probe, no config apply; the status shows `Sync off`, plus `n changes waiting` computed by one probe when the user looks (button state), not by polling | No poll, no refresh; `Sync off` |
| **Sync now** | One immediate flush, then one immediate refresh, whatever the toggle says | **Flush:** save the drawing if it has unsaved changes (the normal save: backup, drawing, details, native model, take-off record, one transaction); then probe and apply a pending config change | **Flush:** write the project record if it has unsaved edits (the normal save); then poll and refresh read views |
| Shared text | Same words in both apps (live-sync "What the user sees") plus `Sync off` | status bar beside `Saved hh:mm`; button in the tool set | status strip; button in the ribbon's Project group |

Both buttons are UI commands in each app: MynaCAD `sync.set {on}` and `sync.now` (engine/host commands, so command line, control channel, MCP and scripts get them: "everything is a command", CLAUDE.md), MynaOps equivalent commands. MynaOps can also drive MynaCAD's through the activation pipe so the **Open MynaCAD** card on Project home can show and set CAD's sync state; whether the two toggles are one shared switch or two independent ones was Q3 (decided: independent, shown on both).

What "realtime" does **not** mean in this design: MynaCAD does not write the `.osp` on a timer. Autosave stays outside the file. Whether Realtime on should also make MynaCAD save the drawing after edits settle was Q4 (decided: no; take-off is recorded on save and on Sync now, as today).

## 3. What moves from MynaCAD to MynaOps

Inventory from the code and docs read for this review. "Config key" refers to section 4.

### 3.1 Moves to the MynaOps project wizard / project settings (MynaOps writes, MynaCAD reads)

| MynaCAD today | Where (read) | Becomes |
|---|---|---|
| Project Wizard window (five pages) and Edit Project | `ProjectWizardWindow.cs`; `project.wizard` in `cmd/wizard.rs` (`project`, `drawing.units`, `level.setup`, `hcwlayers`, `sheetset.standard`, one undo step) | MynaOps **Create project** wizard and **Project settings** page. The engine command `project.wizard` stays (scripts, `aad-cli`, tests) and is used by MynaCAD to *apply* a configuration (4.4), not by a user dialog |
| Project details: title, owner, location, address, pid, siteArea, plotUse, authority, architect, engineer, date, summary | `Drawing::project` map (`crates/doc/src/lib.rs`), `project` command; kept in the drawing (DXF extension data `AAD_PROJECT`) | `config.project`, `config.team` |
| Slab and beam defaults, default floor height | `project` command, `structure` keys | `config.levels.defaults` |
| Floors, levels and heights | `Level {id, name, elevation, height, ...}` (`crates/doc/src/levels.rs`); `level.new/set/delete/activate/fromfloors/setup` | `config.levels` (MynaOps has a Levels page today, `LevelsPage` over `ProjectStore.Levels`, without ids) |
| Drawing unit | `drawing.units`; the wizard in edit mode already refuses `units`, `floors`, `levels` ("chosen when the project is created") | `config.units` |
| Layer sets | `hcwlayers` (`standard`, `permit`, `presentation`, `both`, `all`) | `config.layer_sets` |
| Discipline tabs (sheet sets) and their layer and block patterns | `sheetset.standard`, `SheetSet {name, layer_prefix, layers, blocks, ...}` (`crates/doc/src/sheetset.rs`) | `config.tabs` |
| Sheet numbering scheme | `sheet.scheme`, `sheetindex.*` | `config.sheets` |
| Naming patterns for generated files and drawing numbers | `projectplan` (`ProjectConfig` in `project.json`: naming rules, floors, sets; command `project.plan` / `project.generate`; folder-and-manifest wizard "stays unbuilt") | `config.naming` (the typed model stays in the engine as the schema and validator; its unbuilt folder generator is outside this plan) |
| Title-block data and company details | `companyfile`, `company.pack.import`, `sheet.titleblock`, `sheet.fields`, `sheet.template.*` | `config.titleblock` (project-wide field defaults) and `config.company` (the imported pack); per-sheet field values stay in the drawing |
| Project-wide plot settings | plot style tables, page setup ([plot-styles.md](plot-styles.md)) | **Not inventoried in this review**: which plot settings are project-wide versus per-layout must be listed in stage C before `config.plot` is defined |
| The side-panel **Project** tab (read-only labels plus "Edit project...") | status.md "Project Wizard (WinUI, one drawing)" | Stays as read-only display; "Edit project..." becomes "Edit in MynaOps" (opens MynaOps' Project settings through the pipe) |

### 3.2 What MynaCAD keeps

Drawing tools; layer **editing inside the configured sets** (layers can be created, recoloured, frozen; the configured sets' own layers cannot be deleted or renamed away from the set: "set" membership is by pattern, so this is a policy, see 4.4); level tabs and everything about working **on** a level (activate, trace settings, move/copy to level, milestones), where **levels are model spaces created from the configured list**; drawing tabs (sheet sets) as views; layouts, viewports, plot and batch plot; the sheet index **display** and publish (stage-C decision whether sheet rows themselves are config or drawing data, 4.2); take-off tools and `takeoff.record`; layer mapper for imports; history, revisions register.

### 3.3 What MynaCAD loses

| Removed from MynaCAD (managed mode) | Replacement |
|---|---|
| File > Open, New, Save As **of project files**; recent list; drag-and-drop open of drawings; the native-working-copy-on-open path | MynaOps Open / New project; MynaCAD **Import** |
| File > Project Wizard and Edit Project | MynaOps wizard and Project settings |
| Project settings dialog (`project`, `floors`, `beams`, `levels` edit forms) | MynaOps Project settings and Levels |
| Level and layer-set **configuration** UI: the level settings form's add/delete/elevation/height controls, the "+ Drawing tab" create path, Format > hcwlayers sets, `level.setup`/`sheetset.standard` menus | MynaOps; the engine commands stay for apply and scripts; see Q7 for whether MynaCAD may still add a level |
| File > Restore from Backup (project level) | MynaOps Project database > Restore |
| `Import company details` (company pack) | MynaOps wizard (Team and company page) |

### 3.4 Import and export in MynaCAD (kept)

| | Planned |
|---|---|
| **Import** | DXF and DWG **into the open project** as a new level, a sheet-set annotation, or onto an existing level (the engine already has `level.import`, `level.moveto` with `create`, `project.merge`, `layermap`, `xref.attach`, `underlay.attach`). The dialog asks where. The converted copy is stored **inside the project's `.osp`**, not as a separate `<name>.osp` beside the source: this is the one change to [NATIVE-OPEN.md](NATIVE-OPEN.md), whose conversion code is reused to produce the drawing parts. The source path, size, SHA-256 and import version go in `source.*`-style keys scoped per import (`import.<n>.*`; the existing unscoped `source.*` keys describe a whole-file working copy and would collide). A changed source is offered **Import again** as today, and a re-import backs the project up first (`before-reimport`) |
| **Export** | DXF, DWG, PDF, SVG, PNG (as today: DXF/DWG by Save As with a foreign name, `exchange.export` per level, plot to PDF/SVG/PNG); an export is always a copy and the project stays as it is |

## 4. The project configuration in the `.osp`

### 4.1 Format rules

- Additive only: new `meta` keys with prefix **`config.`**; no table added or altered; `meta.format` `osp` and `meta.version` 1 unchanged (SUITE.md). Readers ignore unknown keys and unknown fields.
- Each key's value is JSON text, UTF-8, at most 256 KiB (the live-sync reader caps meta strings at 4 KiB for status keys only; the config reader must use its own, larger cap and never allocate by a size the file states: "input-derived numbers are hostile", CLAUDE.md).
- Each config key carries `"v": <integer>`; a reader that does not know a key's version treats that part as absent and says so in its report (never a crash).
- **One revision per change.** MynaOps writes all changed `config.*` keys and the bumped `config.rev` in one transaction, which raises `meta.revision` once and writes `sync.last` (live-sync), so the existing change detection carries configuration with no new mechanism.
- Writers: Ops only (2.2). Readers: both.

### 4.2 Keys

| Key | Value (all with `"v":1`) | Source of truth for |
|---|---|---|
| `config.schema` | `{"v":1,"min_reader":1}` | Presence means "this project is configured by MynaOps". Absent = legacy file (4.5) |
| `config.rev` | `{"v":1,"rev":N,"app":"mynaops","time":"…Z"}`; N rises by one per configuration change | What CAD compares with `config.applied.cad.rev` |
| `config.project` | `{title, client, owner, location, address, pid, siteArea, plotUse, authority, date, summary, code, persona}` | Project details (the `Drawing::project` keys, same names) |
| `config.team` | `{architect, engineer, company: {…pack…}}` | Team |
| `config.units` | `{drawing_unit: "mm"\|"cm"\|"m"\|"in"\|"ft", locked: bool}` | Unit; `locked` is set by Ops once CAD reports content (`applied.cad.has_content`) |
| `config.levels` | `{next_cid, one_per_floor, defaults:{height_mm, slab_mm, beam_mm, ceiling_mm, lintel_bottom_mm}, items:[{cid, name, code, elevation_mm, height_mm, slab_mm?, beam_mm?, ceiling_mm?, lintel_bottom_mm?}]}` ordered bottom to top; `cid` is **never reused** | The level list |
| `config.layer_sets` | `{sets:["standard","permit"\|…]}` | Which `hcwlayers` sets exist |
| `config.tabs` | `{items:[{tid, code, name, layer_prefix, layers:[…], blocks:[…]}]}` | Discipline tabs (sheet sets); `tid` never reused |
| `config.sheets` | `{scheme:{…sheet.scheme…}}` | Sheet numbering scheme. Whether sheet rows (the index) stay drawing data is the working assumption; only the scheme is configuration |
| `config.naming` | `{patterns:{file, drawing_number}}` | Naming |
| `config.titleblock` | `{fields:{…defaults…}}` | Project-wide title-block defaults |
| `config.plot` | **Undefined until the stage-C inventory** (3.1) | |
| `config.applied.cad` | **MynaCAD, local**: `{"v":1,"rev":N,"time":"…","result":"ok"\|"partial"\|"failed","has_content":bool,"items":[{"key":"levels","status":"ok"\|"orphans","detail":"…"}]}` | Ops shows "MynaCAD has applied revision N" |
| `config.proposal` | **MynaCAD, local, one-time**: what a legacy drawing already has (4.5) | Adoption |

**Level identity.** `cid` is MynaOps' id. MynaCAD's `LevelId` is allocated by the document (`Drawing::add_level` returns the id; `level.new` has **no id parameter**: read in `cmd/levels.rs`), so the two cannot be forced equal by a parameter today. Design: MynaCAD keeps the binding `cid → LevelId` in the drawing's project map under the key `config.levelmap` (the project map is a string map already stored with the drawing, so no engine struct changes), and the take-off header's `levels` entries gain an optional `config_id` (additive, like `id` and `elevation_mm` in `docs/bridge/LEVEL-IDS.md`). For **adopted** levels `cid` is set to the existing `LevelId`, so existing take-off rows (`level_id`) stay valid. Whether the engine should instead accept an explicit id was Q8 (decided: binding map).

### 4.3 Libraries

`aad-odp` (Rust) gets a `config` module: parse and validate (`ProjectConfig`, caps, unique names case-insensitive, unique cids, bounded counts), read from `Odp`, a `probe`-style read-only reader (like live-sync's `sync::probe`), and a writer used by tests, the CLI and migration tooling (MynaCAD's shell never calls it). `Office.Shared` (C#) gets `ProjectConfig` read and write next to `OdpFile` (the wizard and settings pages call it). Both are tested against the **same JSON fixtures** committed in both repos (as the .NET-written fixture test does for the project file today). `projectplan::ProjectConfig` (typed manifest, floors, elevation derivation `Z(n+1) = Z(n) + H(n)`) is the validator for floors and naming; reuse, do not duplicate.

### 4.4 How MynaCAD applies a change live

MynaCAD notices `config.rev` > `config.applied.cad.rev` (probe, 2.6; at open it applies before showing the drawing). It applies only when **idle** (no interactive command, no block/text edit, no open modal form; otherwise queued and shown as `Configuration update waiting`), as one engine step that is **its own undo entry named "Project configuration"** and is never merged with user edits. A failed part leaves the drawing as it was (the same backup-and-restore pattern as `project.wizard`) and is reported in `config.applied.cad`.

| Config change | Apply (reusing existing commands) | Never |
|---|---|---|
| New level `cid` | `level.new {name, elevation, height, after}`, record `cid → LevelId` | |
| Level renamed (same `cid`) | `level.set` name; level tab label updates; print-block descriptions update (already follow renames, status.md) | Re-create |
| Elevation or height changed | `level.set`; no objects move | Move objects |
| Level removed from the config | If the level has **no objects**, delete it; otherwise keep it and report `orphans: "L3 (120 objects)"`; Ops shows it and the user empties or re-adds it | Delete a level with content |
| Layer set added | `hcwlayers {set}` (creates the layers; idempotent) | |
| Layer set removed | Nothing is deleted; the layers stay and are listed as outside the configured sets | Delete layers |
| Tab added / renamed / mapping changed | `sheetset.new`/`sheetset.set` by `tid` | Delete a tab with annotation |
| Project details, team, titleblock defaults | `project` command; `bplttitleblock` text follows | |
| Sheet scheme | `sheet.scheme` (renumbering is explicit and previewed, as today) | Renumber silently |
| Unit changed after content exists | **Refused** (`units.locked`); Ops should not offer it | Rescale |

Within the configured sets MynaCAD still allows adding a layer, changing colour, freeze and lock; it does not allow deleting or renaming the layers that a configured set defines (Q7 decides the level case).

### 4.5 Existing `.osp` files and drawings that already have levels and settings (migration)

Cases: (a) a `.osp` made by MynaCAD with levels, tabs, project details, a take-off; (b) a `.osp` MynaOps already edits with a `project_doc` that has project info and levels but no ids; (c) a working copy created beside a `.dwg`; (d) a project with no configuration at all.

1. MynaOps **Open project** on a file without `config.schema` shows **Adopt project settings** (a wizard page, nothing is written until Create/Adopt is pressed). It builds the proposal from, in this order: `config.proposal` if MynaCAD has written one; the take-off header `levels` (`id`, `name`, `elevation_mm`, `height_mm`; MynaOps already reads these, `docs/bridge/LEVEL-IDS.md`); `project_doc` (project info and floors); defaults.
2. MynaCAD writes `config.proposal` (local key) when it opens a legacy project: levels with their `LevelId`s, names, elevations, heights; tabs; layer sets present (which `hcwlayers` sets' layers exist); drawing unit; project details; sheet scheme. Reading the drawing is MynaCAD's job because MynaOps cannot parse the native model.
3. Levels in `project_doc` and in the drawing with the same name (case-insensitive) are one entry, the rule the take-off bridge already uses ("A project floor and a level with the same name are one entry"). `cid = LevelId` for adopted levels.
4. The user reviews and presses **Adopt**: MynaOps writes `config.*` in one revision. MynaCAD applies them: everything matches, so the apply is a no-op that records `config.applied.cad`. Nothing in the drawing changes.
5. A project never opened in MynaOps keeps working in MynaCAD standalone only (tests/developers); in managed mode MynaCAD **cannot** start on an unadopted file: it shows "Adopt this project in MynaOps first".
6. Backup: adoption makes a `manual` backup first (MynaOps manager's `Back up now`).

## 5. Removed or changed entry points

| Piece | Today (what was read) | Planned |
|---|---|---|
| MynaHub starting MynaCAD | The Hub status row opens MynaOps, MynaCAD and Myna3D by path and passes nothing (SUITE.md, section 4 and 5). The Hub code was not searched in this review (no start code found by a quick search; the check is stage E) | Hub keeps **opening MynaOps**; its MynaCAD tile starts MynaCAD **without arguments**, which shows the launch screen (2.4). Better: remove the MynaCAD tile, keep MynaOps. Owner decision, Q9 |
| `.osp` double-click | Opens in MynaCAD; the verb **Open in MynaOps** is the second (`O-CoreOp.iss` lines 77 to 80: `.osp` → `OSAS.Project`, verb `ocoreop`, "whichever app registered first stays the default (MynaCAD...)"; MynaHub `installer/suite/README.md`) | Default verb = **MynaOps** ("Open"); the MynaCAD verb is removed. The installer (`MynaOpsCAD-Setup`) must replace the existing default on upgrade: `createvalueifdoesntexist` means an installed default stays unless the upgrade rewrites it (stage E) |
| `.dwg` / `.dxf` associations | MynaCAD's installer registration was **not found/checked** in this review (no `.iss` is tracked in the MynaCAD repo) | If MynaCAD is registered, it hands the file to MynaOps (2.4). Verify in stage E |
| Shortcuts | Start menu entries for MynaOps and MynaCAD from `MynaOpsCAD-Setup` | MynaOps is the suite shortcut; the MynaCAD shortcut is kept but opens the launch screen, or removed (Q9) |
| AI host start | `crates/aad-host/src/osas_ai.rs`: when no host answers, MynaCAD starts `O-CoreOp.exe --ai-host` (found via `OSTUDIO_AI_HOST_EXE`, `ai\host.json`, the folder beside it, the install folder; once a minute at most; `OSTUDIO_AI_AUTOSTART=0` disables). MynaOps `Program.cs` handles `--ai-host` and `--ai-host-stop` before WinUI starts | In managed mode MynaOps is always running and its window hosts the agent, so MynaCAD **does not start it**: the orb says `stopped in MynaOps` or `ready (MynaOps)`. The code path stays for `--standalone`, tests and the MynaOps-closed case: Q10. **Stop** (new, in MynaOps): today a "switch to stop the agent altogether" is listed as planned, not built (SUITE.md "AI agent privacy") |
| Active-project pointer | `ActiveProjectPointer.cs` | Replaced by the leases and the session file; removed in stage E. MynaOps' "Open elsewhere" check moves to the lease |
| Control channel, MCP, `aad-cli`, tests | `--control`, `app.open`, `app.save`, `aad-cli`; many tests open files directly | **Engine commands are unchanged.** Managed mode is a **shell/host policy** (`host.managed`): `app.open` of a project is refused there with the message "projects open through MynaOps". `--standalone` (or `AAD_STANDALONE=1`) keeps today's behaviour, so the existing control-channel, host and CLI tests run unchanged. New tests assert the refusal and the launch contract |
| MynaOps `Export for CAD` / `Import from CAD` file exchange | Exists (`CadExporter`, `CadImportService`) | Unchanged and out of scope; the live file replaces it over time |

## 6. Risks and edge cases

| # | Situation | Design answer |
|---|---|---|
| R1 | **MynaOps closed while MynaCAD is open** | MynaCAD keeps its lease and keeps working and saving (it has the file); it shows `MynaOps closed`, applies no configuration, offers **Open MynaOps**; when a new MynaOps opens the same project it takes its lease and re-attaches (`--ops-pid` was for the old one; MynaCAD re-reads the session file). Closing MynaOps with CAD open asks first (1.1). Any configuration written meanwhile (none can be) is applied on re-attach |
| R2 | **MynaCAD crashes** | The CAD lock disappears with the process. MynaOps notices (no lock) and shows `MynaCAD: not running`; **Open MynaCAD** starts it; MynaCAD finds the recovery snapshot and offers Restore (existing, tested by killing a host process). The file is consistent (SQLite WAL) |
| R3 | **Config change arrives while the user is mid-command** | Queued until idle (4.4); status `Configuration update waiting` |
| R4 | **Two machines, file on a share** | Not enforceable (leases are per PC). Advisory `session.holder` with heartbeat (2.3) warns; SQLite over a network share is unsafe for WAL and is not a supported mode. State it in the user documentation; Q11 |
| R5 | **File moved or renamed while open** | Windows refuses a rename of an open SQLite file, so normally impossible; if it happens (copy then delete), the probe reports `missing` (live-sync) and MynaCAD keeps its in-memory drawing and offers to **save a copy** (an export, the one place a non-project save exists) |
| R6 | **Restore from backup in the database manager while MynaCAD is open** | Refused today; under this design MynaOps first asks MynaCAD to save and close (pipe), takes the maintenance gate, restores, reopens. A restore replaces the project id/revision history: the config revision after restore may be older than `config.applied.cad.rev` (live-sync's `replaced`: revision went backwards). MynaCAD re-applies the restored configuration and reports `result` again; unsaved CAD work is saved before, as a `before-restore` backup |
| R7 | **Foreign file import creating a native working copy inside the project** | 3.4: one transaction inside the project's `.osp` (never beside the source), `before-reimport` backup, `import.<n>.*` provenance, bounded sizes (existing limits: a part inflates to at most 1 GiB, at most 4 000 000 parts; every error a `Result`). The import is slow for big files (apw.dwg 232 212 entities: 9.7 s save into the working copy measured, AUTOSAVE-BACKUP.md): it runs on the worker with progress as the open does today, and leaves the project unchanged on failure |
| R8 | **Two MynaCADs for one project** | Mutex (2.1) makes the second a client of the first |
| R9 | **Config conflicts with content** | Table in 4.4: the drawing wins when deleting would lose objects; Ops shows the orphans |
| R10 | **Ops writes config while the file is `Locked`** | MynaOps' write waits on the 5 s busy timeout, then reports `Locked`; it does not retry silently forever |
| R11 | **Old MynaCAD or old MynaOps opens a configured file** | Additive keys are ignored (live-sync rule). An old MynaCAD can still edit the drawing but would not follow configuration; the installer ships both apps together, and MynaCAD checks `config.schema.min_reader` and shows `Update MynaCAD` if it is newer than it reads |
| R12 | **Hostile config values** | Bounded counts and lengths, unique ids, no `..` in paths, no NaN elevations; a bad config is rejected as a whole by the reader and reported; regression tests per CLAUDE.md "Never crash" (a fixture of hostile configs, like `hostile_dxf_does_not_panic`) |

## 7. Staged plan: work packages

Packages are split so that separate sessions touch different files. Order: **C first** (the contract everyone builds on; briefs in 7a), then **A and B in parallel**, **D** after live-sync is merged and A, B exist, **E** after A and B, **F** throughout and at the end.

| WP | Content | Repo / files it owns (no sharing) | Needs | Acceptance criteria |
|---|---|---|---|---|
| **C. Config schema and libraries** | Section 4 as code: Rust `aad-odp::config` (parse, validate, read-only probe, writer for tests), engine `project.config.apply` (4.4 table, single undo entry, report), `config.proposal` writer, `config.levelmap`; C# `Office.Shared.ProjectConfig` read/write; shared fixture set (valid, old version, hostile); `config_id` in the take-off header | MynaCAD `crates/odp/src/config*.rs`, `crates/engine/src/cmd/projconfig.rs`, `docs/PROJECT-FILE.md`; MynaOps `Office.Shared/ProjectConfig*.cs`, its tests, `docs/PROJECT-FILE.md` | stage-C inventory of project-wide plot settings; live-sync merged (shares `meta` commit path) | Round-trip Rust-written ⇄ C#-read and C#-written ⇄ Rust-read on the shared fixtures; hostile fixtures produce errors, no panic; apply is a no-op on an adopted project; apply on a new level / rename / elevation / removed-with-content / layer set added each behave as the 4.4 table; one revision per Ops write; `cargo xtask ci` and `dotnet test` green |
| **A. MynaOps project home, wizard, launch** | Project home page; Open/New project; create-project wizard (1.2) writing `config.*` through C; Project settings and Levels pages moved onto `config.*` (the existing `LevelsPage` and Settings page); **Open MynaCAD** (starts with the 2.1 contract, activation pipe client); lease (`.ops.lock`) and `session.holder`; AI **Start/Stop**; Database online/offline (1.3); adoption page (4.5); close-both question | MynaOps `BBSDesktop/BBSApp/**`, `Office.Shared` (non-config files), `docs/WINUI-SHELL.md` | C (libraries), the launch contract in this document | New project creates a valid `.osp` with `config.*` and opens; **Open MynaCAD** starts MynaCAD with the right arguments and a second press activates it; Compact/Restore refuse while `.cad.lock` is held and succeed after Database offline; AI Stop and Start change the status strip and the health reply; UI driven by hand and screenshots (the WinUI shell has to be looked at, per CLAUDE.md) |
| **B. MynaCAD managed mode** | `LaunchArgs`: `--managed/--project/--ops-pid/--import/--standalone`; launch screen; mutex + activation pipe + `.cad.lock`; `host.managed` refusing project open; File menu reduced to Save, Import, Export, Print/Plot, History; removal of Project Wizard, project settings, level and layer-set configuration UI (4.4 policy); apply config at open and on change (calls C); status text `MynaOps closed`; AI start suppressed in managed mode; import into project (3.4) | MynaCAD `apps/aad-winui/**`, `crates/aad-host/**`, `crates/engine/src/cmd/file.rs` (import path), `crates/engine/src/nativefile.rs` | C; the launch contract | Starting without `--managed` shows the launch screen; `--managed --project x.osp` opens into the project with configured levels present; second start for the same project activates the first; `app.open` of a project refused in managed mode and unchanged under `--standalone`; a `.dwg` imported lands inside `x.osp` and no file appears beside the source; all existing host/CLI/control tests pass under `--standalone` |
| **D. Sync buttons** | `sync.set {on}`, `sync.now` commands in both apps; the two buttons in the tool sets; `Sync off` state and "n changes waiting"; Ops-to-CAD control through the pipe | MynaCAD `crates/aad-host` sync module, WinUI tool strip entry (small, in B's `apps/aad-winui` area: schedule after B or let B stub it); MynaOps `Office.Shared/OdpSync*`, status strip | Live-sync merged (its `project.sync` and `SyncStatusService`) | Off stops probing and applying (no reads of the file in the process monitor/log); on resumes; Sync now flushes then refreshes in both apps and updates the text; a config revision written by MynaOps appears in an open MynaCAD within the poll interval with sync on and only after Sync now with sync off |
| **E. Migration, installer, entry points** | Adoption end to end (4.5); `.osp` default verb to MynaOps; remove the MynaCAD verb; shortcut and Hub tile decisions (Q9); drop the active-project pointer; AI host start behaviour (Q10); docs for the removed menus | MynaOps `installer/O-CoreOp.iss` and `installer/suite/*` (in MynaHub); MynaCAD `apps/aad-winui/ActiveProjectPointer.cs`; MynaHub status-row tile | A, B | Upgrade from the installed version: `.osp` double-click opens MynaOps; an old MynaCAD-made project adopts with no drawing change (hash of the `drawing` row equal before and after); a legacy file in managed MynaCAD shows "Adopt in MynaOps first" |
| **F. Tests and docs** | End-to-end by hand on one `.osp` with both apps (the live-sync "Run by hand" method); hostile-config tests; the launch contract test; update `docs/status.md` (move rows from planned to implemented only with the tests), `docs/SUITE.md` (three copies), `docs/UI_UX.md`, `docs/NATIVE-OPEN.md`, `docs/AUTOSAVE-BACKUP.md`, `docs/PROJECT_WIZARD.md`, MynaOps `docs/WINUI-SHELL.md`, `docs/PROJECT-FILE.md`, MynaHub `docs/testing/` plans | docs, test scripts | all | Each doc names what was run and what was not (documentation policy); `cargo xtask ci` green |

## 7a. Ready-to-use briefs for the implementation sessions

Each brief can be pasted as the first message of a session. Common rules for every session: read this document, `CLAUDE.md` of the repo, and the live-sync design first; no `unwrap`/`expect`/`panic` in non-test Rust, no
`unsafe`; every crash fix has a regression test; additive `.osp` changes only (new `meta` keys); an app writes only the tables it owns (2.2); engine commands are never changed to enforce managed mode (policy lives in the
host/shell); WinUI work is looked at in a running window (screenshot) and documents say what was run and what was not; own git worktree on a new branch, commit by file name, never `git add -A`, no push, no merge; a separate
`CARGO_TARGET_DIR` per session. Decisions Q1 to Q12 (section 8) are settled; do not reopen them.

### Brief C: configuration schema and libraries (start first)

- **Scope.** Implement section 4. Rust: module `config` in `aad-odp` (types, parse, validate with caps and unique `cid`/`tid`/names, read from `Odp`, read-only probe, a writer for tests and CLI); engine command `project.config.apply` (the 4.4 table, one undo entry "Project configuration", whole-or-nothing, reuses `project`, `drawing.units`, `level.new/set/delete`, `hcwlayers`, `sheetset.new/set`, `sheet.scheme`), `config.levelmap` in the drawing project map, the local keys `config.applied.cad` and `config.proposal` (writer for legacy files: levels with `LevelId`, tabs, layer sets present, unit, details, scheme), optional `config_id` on take-off header `levels` entries. C#: `Office.Shared.ProjectConfig` read and write beside `OdpFile`. First task inside the package: inventory the project-wide plot settings (3.1) and add `config.plot` only if the inventory finds any; write the result into this document.
- **Files / areas.** MynaCAD: `crates/odp/src/config*.rs` (new), `crates/engine/src/cmd/projconfig.rs` (new) and its registration, `crates/engine/tests/`, `docs/PROJECT-FILE.md`. MynaOps: `BBSDesktop/Office.Shared/ProjectConfig*.cs` (new), `Office.Shared.Tests`, `docs/PROJECT-FILE.md`. Shared JSON fixtures committed identically in both repos (valid, old version, unknown fields, hostile).
- **Acceptance.** Rust-written files read by C# and C#-written read by Rust on the shared fixtures; hostile fixtures (huge counts, duplicate ids, NaN elevations, `..` paths, oversize strings) are errors with no panic (regression test named like `hostile_config_never_panics`); one revision per Ops write and `sync.last` set; apply is a no-op on an adopted project; apply on new level, rename, elevation change, removed level with objects (kept, reported as orphan), removed empty level (deleted), layer set added, unit change refused when `units.locked` each tested; `cargo xtask ci` and `dotnet test` green.
- **Do not touch.** WinUI shells, `ActiveProjectPointer`, the AI code, the installer, the live-sync detection code (depend on it, do not edit it), `project.wizard` behaviour (keep it working), existing meta key meanings, any table.

### Brief A: MynaOps project home, wizard and launch

- **Scope.** Project home page; Open project and New project; the create-project wizard of 1.2 writing `config.*` through `ProjectConfig`; move the existing Levels page and the project/persona fields of the Settings page onto `config.*` (keep `project_doc` consistent in the same transaction); **Open MynaCAD** (starts `ODraft.WinUi.exe` with the 2.1 arguments, finds it beside MynaOps or in the install folder, activation-pipe client for `activate`, `close`, `sync.*`, `status`); the `.ops.lock` lease and `session.holder` heartbeat; AI **Start / Stop** (new; reflected in the status strip and the health reply); **Database online / offline** (Q1: ask MynaCAD to close, checkpoint, release leases; Compact and Restore require offline or no `.cad.lock`); the Adopt page (4.5); the close-both question; restore in the manager closes MynaCAD first (2.5).
- **Files / areas.** MynaOps `BBSDesktop/BBSApp/**` (new Views, MainWindow, `Services/`), `Office.Shared` files other than `ProjectConfig*` and `OdpSync*`, `docs/WINUI-SHELL.md`, `docs/PROJECT-FILE.md` sections about the manager.
- **Acceptance.** New project creates a valid `.osp` with `config.*` and opens; Open MynaCAD starts it with the right arguments and a second press activates the running one; Compact and Restore refuse while `.cad.lock` is held and succeed after Database offline; AI Stop then Start change the status strip and `/v1/health`; the adopt flow leaves the `drawing` row hash unchanged and makes a `manual` backup first; pages driven in a window with screenshots (what was not clicked through is listed); xunit tests for the lease, session holder and launch-argument building; `dotnet build` and `dotnet test` green.
- **Do not touch.** MynaCAD repo, the live-sync files (`OdpSync*`, `SyncStatusService`, `ProjectFileWatcher`), `ProjectConfig*` (package C owns them), the take-off page logic, MynaHub, the AI model download code, the CSV export.

### Brief B: MynaCAD managed mode

- **Scope.** `LaunchArgs` options `--managed --project --ops-pid --import --standalone` (also `AAD_STANDALONE=1`); the launch screen (Open MynaOps, then exit after handing over; `.dwg`/`.dxf`/`.osp` arguments forwarded to MynaOps with `--import-pick`); mutex, activation pipe (JSON lines: `activate`, `import`, `close`, `sync.set`, `sync.now`, `status`) and `.cad.lock`; `host.managed` refusing `app.open`/new/save-as of project files with "projects open through MynaOps"; File menu reduced to Save, Import, Export, Print/Plot, History, Open a copy of a backup (Q5); remove Project Wizard, project settings, level and layer-set configuration UI, project Restore from Backup, Import company details (3.3; the engine commands stay); apply configuration at open (before the drawing is shown) and on change when idle, with the status `Configuration update waiting`; `config.proposal` for legacy files and "Adopt this project in MynaOps first"; status `MynaOps closed` and re-attach; AI start suppressed in managed mode (Q10: keep for `--standalone` and for the MynaOps-closed state); **Import into the project** (3.4): conversion inside the project `.osp`, `import.<n>.*` provenance, `before-reimport` backup, never a working copy beside the source.
- **Files / areas.** MynaCAD `apps/aad-winui/**` (LaunchArgs, MainWindow, menus, ProjectWizardWindow removal, SafeFiles, the status bar), `crates/aad-host/**`, `crates/engine/src/nativefile.rs` and the import path in `cmd/file.rs`, `docs/UI_UX.md`, `docs/NATIVE-OPEN.md`, `docs/AUTOSAVE-BACKUP.md`, `docs/PROJECT_WIZARD.md`.
- **Acceptance.** Without `--managed` the launch screen appears; `--managed --project x.osp` opens into the project with the configured levels present; a second start for the same project activates the first; `app.open` of a project is refused in managed mode and unchanged under `--standalone`, and all existing host, CLI and control-channel tests pass under `--standalone`; a `.dwg` import lands inside `x.osp` and no file appears beside the source (test); `cargo xtask ci` green; the window driven by hand with screenshots (launch screen, File menu, import dialog).
- **Do not touch.** `crates/odp/src/config*.rs` and `cmd/projconfig.rs` (package C), MynaOps, the engine command semantics (`project.wizard`, `level.*`, `open`), the take-off record format, the live-sync detection internals (the Sync buttons are package D; leave a stub entry).

### Brief D: Sync on/off and Sync now (after live-sync is merged and A, B exist)

- **Scope.** MynaCAD commands `sync.set {on}` and `sync.now` (engine/host, so command line, control channel and MCP get them) and the two controls in the tool set; MynaOps equivalent commands and ribbon controls (Project group); the `Sync off` state and "n changes waiting" (one probe on demand, no polling while off); Sync now = flush (normal save of unsaved work) then refresh then apply a pending configuration; the Ops-to-CAD requests over the pipe; independent switches shown in both apps (Q3); outgoing writes are never paused; no timer saves (Q4).
- **Files / areas.** MynaCAD the sync module in `crates/aad-host`, `apps/aad-winui` tool-strip entry for the buttons; MynaOps `Office.Shared/OdpSync*`, `SyncStatusService`, status strip and ribbon entry; `docs/live-sync.md` (merged location).
- **Acceptance.** With sync off the process makes no reads of the file by the detector (counter/log test) and applies nothing; on resumes; Sync now flushes then refreshes in both apps and updates the text; a configuration revision written by MynaOps appears in an open MynaCAD within the poll interval with sync on and only after Sync now with sync off; tests plus both apps run by hand on one `.osp` (what was seen is recorded as in live-sync "Run by hand").
- **Do not touch.** The `config.*` schema, the lease and launch code, the file format, autosave.

### Brief E: migration, installer and entry points (after A and B)

- **Scope.** End-to-end adoption of legacy files (4.5) including files with a working-copy history; `.osp` default verb becomes MynaOps and the MynaCAD verb is removed, rewritten on upgrade (the existing `createvalueifdoesntexist` keeps an old default); MynaCAD shortcut and the MynaHub tile lead to the launch screen (Q9); check and fix `.dwg`/`.dxf` registrations (not verified in the review); remove `ActiveProjectPointer` and move MynaOps' "open elsewhere" check to the lease; remove or hide the `nativefile` beside-the-source path from the UI; installer upgrade test from the installed version.
- **Files / areas.** MynaOps `BBSDesktop/installer/O-CoreOp.iss`; MynaHub `installer/suite/*` and the status-row tile (MynaHub repo, own worktree); MynaCAD `apps/aad-winui/ActiveProjectPointer.cs`; docs `docs/SUITE.md` (three copies), `RELEASE_AND_PACKAGING.md`, MynaOps `docs/PACKAGING.md`.
- **Acceptance.** After upgrade an `.osp` double-click opens MynaOps; a project made by an older MynaCAD adopts with the `drawing` row hash identical before and after; a legacy file in managed MynaCAD shows "Adopt this project in MynaOps first"; no pointer file is written; Compact and Restore still refuse correctly using the lease.
- **Do not touch.** The schema, the sync code, the drawing engine.

### Brief F: tests and documentation (runs alongside, finishes last)

- **Scope.** The two-app end-to-end run by hand on one `.osp` using the live-sync method; a launch-contract test; hostile-config tests (with C); a test that `--standalone` keeps every existing control-channel test valid; keep `docs/status.md` rows planned until their tests exist, then move them; update `docs/SUITE.md` (three copies kept identical), the MynaHub `docs/testing/` plans, and this document's "verified" section with what was actually run.
- **Files / areas.** Docs and test scripts only: both repos' `docs/`, MynaOps `BBSDesktop/tests/e2e/` new files (leave the existing e2e scripts with line-ending noise alone), MynaHub `docs/testing/`.
- **Acceptance.** Each affected doc names what was run and what was not; the status matrix matches the code; `cargo xtask ci` and `dotnet test` green.
- **Do not touch.** Production code.

Order: C; then A and B in parallel (they share nothing but the contract and package C's libraries); D after live-sync is merged and A and B are in; E after A and B; F alongside and last.

## 8. Decisions on the open questions (all decided by owner 2026-11-15: accept recommendation)

| # | Question | Decision (the recommendation, accepted) |
|---|---|---|
| Q1 | What does "DB server start/stop" mean? Today there is no server: the project database is a SQLite file and the manager only has maintenance commands | **Decided by owner 2026-11-15: accept recommendation.** **Database online/offline** in MynaOps (1.3): offline = MynaCAD closed, WAL folded in, leases released, so Compact, Restore and copying the file are safe. Not a network database server; that would be a new product (and the cloud database belongs to MynaHub/Neon) |
| Q2 | May MynaCAD run alone as a drafting scratchpad that is not a project? | **Decided by owner 2026-11-15: accept recommendation.** **No** in the shipped app (your words: no individual open). Direct start shows the launch screen. Keep `--standalone` hidden for tests and developers. A scratchpad can be added later as an *Untitled project* created in MynaOps |
| Q3 | Is Realtime sync one shared switch or one per app? | **Decided by owner 2026-11-15: accept recommendation.** **Independent per app, visible in both** (each app decides whether it interrupts its own user) |
| Q4 | Should Realtime on make MynaCAD save the drawing and record the take-off automatically after edits settle? | **Decided by owner 2026-11-15: accept recommendation.** **No** in the first version: Save and Sync now do it, as today, because every changing save makes a backup copy of the whole file (619 ms for 239 MB measured). Revisit after live-sync is used |
| Q5 | Should MynaCAD keep **Open a copy** of a backup (read-only look at an earlier state)? | **Decided by owner 2026-11-15: accept recommendation.** Yes, harmless; project **Restore** moves to MynaOps only |
| Q6 | Two projects at once: one MynaOps window per project and one MynaCAD per project? | **Decided by owner 2026-11-15: accept recommendation.** Yes, allowed, with per-project leases. If simpler is preferred: one project per MynaOps session, closing the old CAD first |
| Q7 | Can MynaCAD still add or delete a level, or only MynaOps? | **Decided by owner 2026-11-15: accept recommendation.** **Only MynaOps** (D3: single writer). CAD can still change a level's view, traces and work on it; objects can be moved between configured levels. A level that exists in the drawing but not in the config (legacy/orphan) is shown with a warning and Ops adopts or removes it |
| Q8 | Level identity: binding map (`cid → LevelId`, 4.2) or make the engine accept an explicit id? | **Decided by owner 2026-11-15: accept recommendation.** **Binding map** now (no engine change; adopted levels use `cid = LevelId`). Revisit if the map proves awkward |
| Q9 | Does the MynaHub tile and the MynaCAD Start menu shortcut stay? | **Decided by owner 2026-11-15: accept recommendation.** **Keep the shortcut and the Hub tile**, both leading to the launch screen, which starts MynaOps. Alternative: remove them (fewer ways to start wrong) |
| Q10 | Does MynaCAD keep starting the headless AI host when MynaOps is not running? | **Decided by owner 2026-11-15: accept recommendation.** **Keep it only for `--standalone` and for the "MynaOps closed" state**, off by default in managed mode (the owner wants AI start/stop in MynaOps) |
| Q11 | Is a project file on a network share or OneDrive supported? | **Decided by owner 2026-11-15: accept recommendation.** **Not supported in this phase**; document it (SQLite WAL is unsafe on shares; OneDrive was not tested, AUTOSAVE-BACKUP.md). One PC, one file |
| Q12 | What if the wizard is edited later: may units or floor structure change after content exists? | **Decided by owner 2026-11-15: accept recommendation.** Units: **locked** once CAD reports content. Levels: add/rename/re-elevate freely; delete only when empty (4.4) |

## 9. What was verified in this review, and what was not

Read, not run: no program was started and no test was executed for this document (it is docs only).

Read in the code: `apps/aad-winui/LaunchArgs.cs`, `ActiveProjectPointer.cs`; `crates/aad-host/src/osas_ai.rs` (the AI start logic and its environment variables); `crates/engine/src/cmd/wizard.rs` (what `project.wizard` runs and that edit mode refuses units/floors/levels); `crates/engine/src/cmd/levels.rs` (`level.new` has no id parameter); `crates/doc/src/levels.rs` (`Level`, `LevelId(u32)`), `crates/doc/src/sheetset.rs`; `crates/odp/src/project.rs` (tables); MynaOps `Program.cs` (`--ai-host`, `--ai-host-stop`), `Office.Shared/OdpFile.cs` (`Create`, `Open`, `project_doc`), `Views/LevelsPage`, `SettingsPage`, `installer/O-CoreOp.iss` (the `.osp` registration), `docs/PROJECT-FILE.md`, `docs/bridge/LEVEL-IDS.md`; and these docs: SUITE.md, status.md, PROJECT_WIZARD.md, projects.md, NATIVE-OPEN.md, AUTOSAVE-BACKUP.md, MynaHub `installer/suite/README.md`, and `docs/live-sync.md` from branch `claude/osp-live-sync-aa35c6`.

Not checked: whether MynaOps starts another program anywhere (only `Program.cs`, `MainWindow.Ai.cs` references were looked at, and `BBSApp/*.cs` for a single-instance mutex, none found); MynaHub's code that starts MynaCAD; MynaCAD's installer registration for `.dwg`/`.dxf` (no `.iss` tracked in the MynaCAD repo); which plot settings are project-wide; the live-sync branch's code (only its design document); that the named pipe and lock-file designs work (design only); the UI of any planned screen.

