# Myna suite: names, roles, files, links and how it works live

**Canonical copy:** the MynaHub repository, `docs/SUITE.md`. MynaOps (`MynaOps`) and MynaCAD (`MynaCAD`) carry an identical copy at `docs/SUITE.md` so a developer in any one repo knows what the others are, what they are called and how they connect. If you change one, change all three (links are written as repo and path, not as relative links, so the copies can stay identical). Updated 2026-11-11. The checkout folders were renamed on 2026-11-11: `O-DRAFT-main` to `MynaCAD`, `O-COREOP` to `MynaOps`, `O-STUDIO` to `MynaHub`, `o-ui` to `MynaUI` (the git remotes keep their old names; the sibling folders `MynaHub`, `MynaOps`, `MynaCAD`, `MynaUI` must stay beside each other).

## 0. What changed on 2026-11-11 (owner decisions)

| Decision | State |
|---|---|
| The **Myna launcher** (`O-Studio.Launcher.exe`: tray icon, window, app tiles, AI Core host, launch requests, the "Myna" shortcuts) is **removed**. Nothing starts from it | Implemented (branch `claude/no-launcher`). `MynaHub-Setup` removes an installed launcher on upgrade |
| **MynaHub** keeps every login (company sign-in, users, admin password, roles) and its own database (the company file `.osas`; Neon cloud sync is MynaHub only). MynaHub has **no AI**: no assistant bar, no AI status, no AI settings passed to the apps it starts | Implemented |
| The **AI agent** (Olmo 3 7B through llama.cpp `llama-server`, loopback `127.0.0.1:8742`) is **hosted by MynaOps** and serves **MynaOps and MynaCAD** only. A MynaOps window hosts it; without a window MynaOps runs it headless (`O-CoreOp.exe --ai-host`), which **MynaCAD starts** when no host answers and which ends by itself when neither app is open | Implemented |
| The **project database manager** for the shared `.osp` lives in **MynaOps** (Project > Project database, also File > Project database) | Implemented |
| MynaCAD and MynaOps stay independent of MynaHub (no sign-in, no link) | Unchanged since 2026-11-08 |

## 1. Names

**Myna** is the name of the suite (the family of apps); there is no app called Myna any more. Each app is Myna plus a use-case word: **MynaHub** is the hub (company, people, approvals; was Kivo); **MynaCAD** is drafting (was Tren); **MynaOps** is operations: quantities, estimates and billing (was Falcon); **Myna3D** (was MynaMod, earlier Finch) is 3D / BIM modelling (planned; was Finch). The older bird names (Kivo, Tren, Falcon, Finch) and the O- names are retired from user text. Identifiers keep their old spellings (install folders, exe names, `OSTUDIO_*`, the stored origin stamp `"app":"Tren"`). Marks: the three-circle suite mark on each app's colour; the orb is the AI agent's mark inside MynaOps and MynaCAD (MynaHub `docs/BRANDING.md`).

| Name to use | What it is | Colour | Repository / folder | Identifiers that keep old spellings |
|---|---|---|---|---|
| **MynaHub** | Office and projects; **suite hub**: company file, sign-in, users, roles, approvals and revisions, Import from MynaOps (CSV); the one app synced to the cloud (Neon). No AI | Ember `#B83E16` | `MynaHub` | `O-Studio.exe`, `aos`, `AOSENC1`, `office-core`, `office.dll` |
| **MynaOps** | Quantities, estimates, rate books, bar bending, BOQ, purchase orders, running bills, scheduling; **hosts the AI agent** for itself and MynaCAD; **project database manager** | Dusk `#5630A0` | `MynaOps` | `O-CoreOp.exe`, `AQCCore.exe` (old), `bbs_engine.dll`, `.bbsproj` (old project files), `aqc` |
| **MynaCAD** | 2D CAD: drafting, DXF/DWG, layouts, plotting, architecture tools and the take-off tools; a client of the AI agent | Midnight `#102C80` | `MynaCAD` | `ODraft.WinUi.exe`, `aad-*` crates, `aad-host.dll`, `aad-winui` |
| **MynaUI** | Shared WinUI themes, marks and chrome (`O.Ui` assembly; repo `MynaUI`; **Hcw.Ui / hcw-ui retired**) | | `MynaUI` | `O.Ui` |
| **Myna3D** | Planned, not built | Grove `#166040` | `Myna3D` (folder; was `O-Form`; the app was named MynaMod before 2026-11-11) | |

The former **O-Dynamo** companion (a Dynamo fork) is not part of the suite: MynaCAD builds and runs without it. Its optional hook (the `ODRAFT_GRAPH` build flag when a sibling `O-Dynamo` folder exists, and the `ODraft.Dynamo` pipe behind `companion.*`) stays in MynaCAD, inactive. Its GitHub repository was left as it is.

**Suite wire (implementation, not user-facing):** shared state lives under `%LOCALAPPDATA%\O-Studio\` (`suite.json`, `active-project.json`, `appearance.json`, `neon.env`, `selected-project.json`, `ai\auth.token`, `ai\host.json`, `ai\host.log`). MynaHub starts MynaOps, MynaCAD and Myna3D by path and passes **nothing** in their environment (no company, no ticket, no project, no AI settings). Project file meta format id is **`osp`** v1; the active-project pointer uses **`studio-active-project`** v1. Constants: `Office.Shared.SuiteWire` (.NET), `Osas.Core.SuiteWire` (MynaHub).

**AI agent wire (unchanged contract):** `GET /v1/health`, `POST /v1/chat`, `POST /v1/completion` on `127.0.0.1:8742`, bearer token from `%LOCALAPPDATA%\O-Studio\ai\auth.token` (created by the host). Overrides: `OSTUDIO_AI_HOST`, `OSTUDIO_AI_PORT`, `OSTUDIO_AI_TOKEN`, `OSTUDIO_AI_ROLE` (`client` or `host`). Added 2026-11-11, all optional: `ai\host.json` (`{format:"myna-ai-host", version:1, port, pid, mode:"window"|"headless", exe, started_utc}`, written by the MynaOps that hosts, removed when it stops), `host` and `pid` in the health reply, `OSTUDIO_AI_DIR` (moves the `ai` folder; tests), `OSTUDIO_AI_HOST_EXE` (MynaCAD: which `O-CoreOp.exe` to start), `OSTUDIO_AI_AUTOSTART=0` (MynaCAD: never start MynaOps), `OSTUDIO_AI_IDLE_SECONDS` (headless host idle time, default 180).

Other names:

| Term | Meaning |
|---|---|
| **Company file** | One file per practice, `.osas` (earlier `.adms` opens too). SQLite through the `office-core` Rust engine, optionally encrypted (`AOSENC1`). Chosen, created and signed in to in **MynaHub** only. MynaOps and MynaCAD do not open it |
| **Project file** | `.osp`. One SQLite file per project holding the drawing, the take-off and the project record. Format name `osp`, version 1. Changes to it are additive only: new meta keys or new tables, never an altered table. (`.odp` was the working name and clashed with LibreOffice Impress; it is not used) |
| **Take-off** | Quantities measured from a drawing. Recorded by MynaCAD into the project file; read by MynaOps |
| **Revision** | The counter inside a project file (`meta.revision`). Every save by any program is one revision. Apps compare revisions to know someone else wrote |
| **Active-project pointer** | `%LOCALAPPDATA%\O-Studio\active-project.json`, written by MynaCAD: which project file is open there. MynaOps uses it to open the same file and, in the project database manager, to know MynaCAD has the file open |
| **Shared project file** | One `.osp` opened in **MynaCAD** and **MynaOps**. Double-click opens it in MynaCAD; **Open in MynaOps** is a separate verb. MynaCAD writes the drawing and take-off; MynaOps writes quantities, the estimate, bills, and the project maps and layouts |
| **Backups of a project file** | `<name>.backups\<name> <UTC yyyyMMddTHHmmssZ> <reason>.osp` beside the file, each a whole project file. MynaCAD makes one before every save that changes the drawing (`save`), before a re-import or a restore, and prunes to its `BACKUPCOUNT` (default 5, 90 days); MynaOps' manager adds `manual` ones (MynaCAD `docs/AUTOSAVE-BACKUP.md`) |
| **Persona** | In MynaOps, Project Manager/PMC or Contractor: each has its own letterhead and numbering |
| **AI agent** | Olmo 3 7B Instruct (Q4_K_M GGUF in `%LOCALAPPDATA%\HumanCentricWorks\models\`), served by `llama-server` (llama.cpp b11976, bundled in MynaOps' `runtime\`) behind MynaOps' `AiCoreListener` on `127.0.0.1:8742`. Runs on this computer only. **MynaOps** hosts it; **MynaCAD** is a client; MynaHub does not use it |

## 1a. The workflow (decided 2026-11-08, AI host changed 2026-11-11)

**MynaCAD and MynaOps are independent of MynaHub.** They do not sign in to MynaHub, ask it for a session, or need it running. They share one **local `.osp` file**, which is their database; it stays on this PC and is never synced. **MynaOps exports a CSV; MynaHub imports it** for estimates and running bills, and that data becomes part of the company database. **Approvals and revisions live in MynaHub.** Only MynaHub is synced to the cloud.

| App | Owns | Does not need |
|---|---|---|
| **MynaCAD** | The drawing and its take-off rows, written into the local `.osp` through its own engine; rolling backups and crash recovery of its drawings | MynaHub, a sign-in, a network; MynaOps for anything but the AI agent |
| **MynaOps** | Quantities, rates, the estimate, read and written in the same local `.osp`; **Export for MynaHub (CSV)**; the **AI agent host**; the **project database manager** | MynaHub, a sign-in, a network |
| **MynaHub** | Company sign-in, users, roles and the admin password; the company file; estimates, running bills, **approvals, revisions**; **Import from MynaOps (CSV)**; the only cloud sync (Neon) | An AI agent, MynaOps or MynaCAD running |

How the pieces connect:

- **Project data.** MynaCAD and MynaOps read and write the same `.osp` on this PC. Nothing in it is pushed anywhere (`Osas.Core.ProjectFilePolicy`; `OSAS_PUSH_PROJECT_FILES=1` is an opt-in that is off).
- **CSV.** MynaOps writes `osas-falcon-csv/1` (20 columns; rows `meta`, `estimate`, `measured`, `takeoff`, `total`; UTF-8 with BOM, RFC 4180, CRLF). MynaHub previews it and imports it in one transaction onto estimate versions and draft running bills. Contracts: MynaHub `docs/contracts/falcon-csv.md` and `docs/contracts/falcon-csv-import.md`; a running bill locks its measured-to-date once certified.
- **AI agent.** A MynaOps window hosts it in-process when the port is free; a second MynaOps window is a client. When only MynaCAD is open and the user asks something, MynaCAD starts `O-CoreOp.exe --ai-host` (no window; found through `OSTUDIO_AI_HOST_EXE`, `ai\host.json`, the folder beside MynaCAD's, or `%LOCALAPPDATA%\Programs\O-CoreOp`), shows "starting MynaOps…" and sends the question when the host answers; the UI never waits on it. The headless host is one per Windows session (a mutex), loads the model on the first question, and ends when no MynaCAD or MynaOps process has run and nothing has asked for 3 minutes, or on `O-CoreOp.exe --ai-host-stop`. The MynaOps and MynaCAD Setup stops a headless host before it replaces or removes files.
- **Retired 2026-11-08:** MynaHub's session broker, the database service, the sign-in ticket MynaHub passed to the apps it started, and the locked project. MynaHub `docs/contracts/session-broker.md` is kept only as history.
- **Retired 2026-11-11:** the Myna launcher, its AI Core hosting, `launch-request.json`, `launcher-status.txt`, the tray icon and the "Myna" shortcuts; MynaHub's assistant bar and AI status.
- **Design note:** MynaHub `docs/WORKFLOW-KIVO-HUB.md`.

## 2. Hierarchy

```
MynaHub (company sign-in, users, roles, practice records, estimates, bills, approvals, revisions, Neon sync)   <- which company, who is signed in
 |
 +-- Company file (.osas)                                 <- the practice: clients, projects, users, accounts
 |     |
 |     +-- Project (ProjectRef: company_uuid + project_id)  <- identity shared by all apps
 |           |
 |           +-- Project file (.osp)                        <- the working data of one project (on this PC)
 |                 |-- drawing(s)  (DWG/DXF bytes, unchanged)
 |                 |-- take-off    (headers, rows, annotated PDFs)
 |                 |-- project record (estimate, bills, tasks, quantities, POs)
 |                 |-- markups, history, change log
 |
 +-- MynaOps   reads/writes the project record and take-off view in the .osp; exports the CSV MynaHub imports; hosts the AI agent
 +-- MynaCAD   writes the drawing and take-off in the .osp; asks the AI agent MynaOps hosts
 +-- Viewer (planned: tablet/phone)  views and marks up the synced project
```

Rule: **a project's data lives in its `.osp`; the practice's data lives in the company file; nothing else is a source of truth.** Cloud (**Neon** — Postgres and Object Storage) is a master copy that MynaHub syncs; MynaOps and MynaCAD never hold cloud keys.

## 3. Roles

Users belong to the company. Roles are in `office-core` (`auth.rs`):

| Role | Can |
|---|---|
| **Owner** | Everything: settings, users, backup, post, close periods, issue, view audit |
| **Principal** | View, issue |
| **Accounts** | View, issue, post |
| **Document editor** | View and edit documents |
| **Auditor** | View, and view the audit trail (read only) |

Users are added, re-roled and re-passworded in **MynaHub** (Office > Users), each change asking for the signed-in person's password again; the last active owner cannot be removed. MynaOps and MynaCAD have no sign-in. Firm details for MynaCAD title blocks are an exported pack, not a live company-file session. The project's persona (PM/PMC or Contractor) is separate from the user's role.

## 4. What each app has on screen

### MynaCAD (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Menu bar | File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Architecture, Window, Help |
| Top tool strip (row 1) | Layer box, **Draw / Modify / Annotate** (horizontal), architecture tools, gear (View > Customize Top Bar) |
| Take-off strip (row 2) | **Take-off tools only** (Ext wall, Int wall, Beam, … Record, Map layers, phase pickers) while **Take-off** is on in Classic workspace |
| Right | Layers / Properties / Blocks tabbed stack |
| Bottom bar | Snap/ortho/polar toggles, coordinates, units, **Take-off toggle** (next to Nameplate); the AI agent mark, whose tooltip shows the agent's state ("ready (MynaOps)", "starting MynaOps…", "needs MynaOps", "the model is not installed") |
| File | New, Open, Save, Save as, Close, Export, Print/Plot, History, Restore from Backup, and **Import company details** (`company.pack.import`). The project file is the `.osp` shared with MynaOps. Company details for title blocks are a JSON pack exported from MynaHub (Firm settings, **Export for MynaCAD**). MynaCAD does not call MynaHub |
| Command line | Every tool is also a typed command, e.g. `takeoff.record` |

Details: MynaCAD `docs/UI_UX.md`.

### MynaOps (WinUI shell, C++ engine)

| Region | Contents |
|---|---|
| Main ribbon | One text tab per section: Project, RCC steel, Civil BOQ, Schedule, Contracts, Stores, Outputs (Ctrl+1 to Ctrl+7). Office, Accounts and HR live in MynaHub |
| Sub ribbon | One text tab per page of the open section. Project: Dashboard, Levels, Take-off (MynaCAD), **Project database** |
| Take-off page | Reads the take-off from the open `.osp`; shows "MynaCAD updated the take-off" when MynaCAD writes; Import brings it into the estimate |
| Project database page | The shared `.osp` as a database: format and version, revision, size (and write-ahead log), free space, folder, rows per table; **Check integrity**, **Compact**, **Back up now**, the backups (newest first) and **Restore selected**; who else has the file open (MynaCAD through the active-project pointer while it runs, or any program holding the file). Compact and Restore are refused while the file is open elsewhere. Nothing changes the format |
| Layout page | Outputs > Layout. The left side is the project map and its layouts. The right side lists print blocks saved in the open `.osp` by MynaCAD. Insert places the related print block, Update nameplate stores the title fields in the project document, and Export PDF writes a sheet record |
| File | Open project (the shared `.osp`), Save, Take-off, Import from CAD, Export for CAD, Undo last CAD import, **Project database**, Exit |
| Edit | Project settings, Engineering settings, Cost percentages, **Assistant model** (where the model is, who hosts the agent, Download the model with size and SHA-256 checked, Locate a model file) |
| Bottom shell | Full-bleed status row + floating Ask pill (`O.Ui` `SuiteBottomStatusBar`, `SuiteAskBar`); the status row shows **AI: ready / off / model not installed / runtime missing** (checked every 10 s; click for Assistant model) |
| Assistant (Olmo) | Hosted by MynaOps (this window, or the headless MynaOps when another one already serves `:8742`) |

Details: MynaOps `docs/WINUI-SHELL.md`.

### MynaHub (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Sign-in / company | New or open company file (`.osas`) **in MynaHub**; sign in; remember the path in `suite.json`; admin password and **Users** |
| Left pane | Practice (Pulse, Clients, Projects, enquiries, site records, permits, proposals, meetings); Documents (letters, templates); Billing (invoices, receipts, reconciliation); Books (expenses, income, salaries, assets, journals); Reports and tax; Office (firm profile, staff, leave, attendance, library, security, backup); **Suite** (dashboard, **Cloud sync**, approvals) |
| Shortcuts | Ctrl+N new record, Ctrl+S save, Ctrl+P print, Ctrl+F search, Ctrl+K go to page |
| Bottom shell | Status row (`O.Ui`); **opens MynaOps, MynaCAD, Myna3D** from the status row (by path, nothing passed) |
| Cloud sync | Suite > Cloud sync: Neon settings, Test connection, **Start sync** (background sync runs in MynaHub while it is open), per-project push and pull |

MynaHub has no Ask bar and no assistant.

## 5. How they are linked, and how it works live

| Link | Mechanism | Latency |
|---|---|---|
| MynaHub -> apps | Starts the app by path; nothing is passed | on launch |
| MynaCAD -> MynaOps | Both open the **same `.osp`**. MynaCAD's `takeoff.record` writes headers/rows/annotated PDF in one revision; MynaOps polls `meta.revision` every 2 s and notifies | about 2 s, no server |
| MynaCAD -> MynaOps (AI agent) | Loopback HTTP to `127.0.0.1:8742` with the token; MynaCAD starts `O-CoreOp.exe --ai-host` when nothing answers | first question: MynaOps start plus model load |
| MynaOps -> MynaHub | **Export for MynaHub** writes the CSV; MynaHub imports it | on export and import |
| MynaHub -> MynaCAD (company details) | Firm settings > **Export for MynaCAD** writes `osas-company-pack` v1 JSON. MynaCAD **Import company details** applies it to title blocks. No live session | on import |
| MynaCAD -> MynaOps (which file) | The same `.osp`, opened in each app. Active-project pointer: `{format:"studio-active-project", app, drawing, project, updated_utc}` | on drawing change |
| Two programs, one file | SQLite WAL, immediate write transactions, 5 s busy timeout: writers take turns, every commit is exactly one revision, readers never block (tested with 4 writers x 40 commits) | immediate |
| Machine -> machine | MynaHub syncs with **Neon** (MynaHub `docs/CLOUD-NEON.md`); project files stay on this PC unless the push opt-in is set | Postgres revision + Object Storage |

Both apps share one format library: Rust `aad-odp` (MynaCAD) and .NET `Office.Shared.OdpFile` (MynaOps), kept compatible by tests that read each other's files. MynaOps' `Office.Shared.ProjectDatabase` (the manager) only reads, compacts with `VACUUM`, and copies with `VACUUM INTO`; it adds no table and does not move the revision.

Saving: MynaCAD writes the drawing back into the `.osp` only when its bytes changed (so a take-off-only change does not touch the drawing), and records take-off changes as differences only.

## 6. Where to read next

| Topic | Document |
|---|---|
| Project file format and sync plan | MynaHub `docs/ODP-PROJECT-FILE-AND-SYNC.md` (filename keeps the working name) |
| Project file backups, autosave, recovery | MynaCAD `docs/AUTOSAVE-BACKUP.md` |
| Project hierarchy and security | MynaHub `docs/PROJECT-HIERARCHY-AND-SECURITY.md` |
| Suite architecture and integration map | MynaHub `docs/suite-architecture.md`, `docs/suite-hub.md` |
| Company file ABI | MynaHub `docs/office-abi.md` |
| MynaCAD UI, commands, performance | MynaCAD `docs/UI_UX.md`, `docs/PERFORMANCE.md` |
| MynaOps shell, model, take-off | MynaOps `docs/WINUI-SHELL.md`, `docs/LOCAL-MODEL.md`, `docs/PROJECT-FILE.md` |
| Neon sync | MynaHub `docs/CLOUD-NEON.md` |
| Shared WinUI chrome | MynaUI (`O.Ui`) — `SuiteBottomStatusBar`, `SuiteAskBar`, theme tokens |
| ShilpiDB → `.osp` migration (historical) | Each app repo `docs/MIGRATION-OSP.md` — do not use **ShilpiDB** in new user text |
| Installers | **Two installers:** `MynaHub-Setup-<version>.exe` (MynaHub; removes the Myna launcher of a 1.7.x install) and `MynaOpsCAD-Setup-<version>.exe` (MynaOps and MynaCAD together, because they depend on each other; bundles the llama runtime; optional model download task). Build with MynaHub `installer/suite/build-all.ps1`, see `installer/suite/README.md`. Per-app: MynaHub `docs/packaging.md`, MynaOps `docs/PACKAGING.md`, MynaCAD `docs/RELEASE_AND_PACKAGING.md` |
| Testing | MynaHub `docs/testing/README.md` (plans for every app, the connected apps, bug process and log) |
| Brand (palette, the mark, the AI agent orb, type) | MynaHub `docs/BRANDING.md`; artwork and colour files in `MynaUI` (`brand/`, `Themes/SuiteAccent.*.xaml`) |

## 7. Conventions

- **App names (user-visible):** MynaHub, MynaOps, MynaCAD, Myna3D, and MynaUI. Spell exactly. Myna3D is planned. **Myna** alone is the suite's name, never an app (the launcher that was called Myna was removed on 2026-11-11). Do not use the earlier names O-Studio, O-CoreOp, O-Draft, O-Form, Kivo, Tren, Falcon or Finch in menus, dialogs, or user-facing sentences. Those names remain ticket ids, install folders, and executable names.
- **Suite hub:** say **MynaHub**. Do not say OSAS or "the OSAS suite" in user text.
- **AI:** say **the AI agent** (the model is **Olmo 3 7B**). It runs in **MynaOps**; MynaCAD uses it; MynaHub has none.
- **Never in user text:** AQC, AOS, AAD, AADT, ShilpiDB, Tulu, **OSAS** as a product name, **OSAS.exe**, "the launcher", "AI Core". Legacy exe names (`AQCCore.exe`, `aad-host.dll`) belong in migration or identifier columns only.
- **Files:** project file = **`.osp`**; company file = **`.osas`**. Do not call the project file ODP or `.odp` in user text.
- **Take-off:** hyphenated **Take-off** (not takeoff or take off) in UI and documentation.
- **Company file** vs **project file:** practice-wide data lives in the company file; one project's drawing, take-off, and estimate live in its `.osp`.
- New project data goes in the `.osp` schema with a change-log row, not a sidecar file, and only as new meta keys or new tables (MynaOps and MynaCAD share the file).
- Apps do not call each other; they meet in the `.osp`, the company file, the active-project pointer, or the AI agent on loopback.
- Cloud keys and Neon credentials live only in **MynaHub**; the Olmo host process only in **MynaOps**. MynaCAD holds neither.

## AI agent privacy

The AI agent (Olmo 3 7B) runs on this computer and is hosted by MynaOps; nothing leaves the PC. Its token is in `%LOCALAPPDATA%\O-Studio\ai\auth.token` and it listens on loopback only. The privacy panel that the removed launcher had (`ai-privacy.json`) went with it. **Planned, not built:** a switch to stop the agent altogether, per-app permissions for MynaCAD and MynaOps, and a privacy panel in their Ask bars.
