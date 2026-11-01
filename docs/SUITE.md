# O-Studio suite: names, roles, files, links and how it works live

**Canonical copy:** the O-Studio repository, `docs/SUITE.md`. Each app repository carries an identical copy at `docs/SUITE.md` so a developer in any one repo knows what the others are, what they are called, what their toolbars hold and how they connect. If you change one, change all four. Updated 2026-10-31.

## 1. Names

The suite is the **O-Studio family of apps** (O-Studio, O-CoreOp, O-Draft, O-UI). **O-Studio** is the hub: company sign-in, users, and the Neon sync settings. The **launcher** hosts the Olmo AI Core, runs the Neon sync, signs in, loads the project and starts the apps. A name below is the name to use in text, menus and installers. Code identifiers keep older spellings because files already use them; they are identifiers, not names.

| Name to use | What it is | Colour | Repository / folder | Identifiers that keep old spellings |
|---|---|---|---|---|
| **O-Studio** | Office and projects; **suite hub** (company file, users, launch O-CoreOp/O-Draft, approvals, Neon `.osp` sync settings) | Flame Orange `#FF5B04` | `O-STUDIO` | `aos`, `AOSENC1`, `office-core`, `office.dll` |
| **O-CoreOp** | Quantities, estimates, rate books, bar bending, BOQ, purchase orders, running bills, scheduling | Wild Sand `#E4EEF0` | `O-COREOP` | `AQCCore.exe` (old), `bbs_engine.dll`, `.bbsproj` (old project files), `aqc` |
| **O-Draft** | 2D CAD: drafting, DXF/DWG, layouts, plotting, architecture tools and the take-off tools | Dark Slate Blue (Mirage) `#16232B` | `O-DRAFT` | `aad-*` crates, `aad-host.dll`, `aad-winui` |
| **O-UI** | Shared WinUI themes, marks and chrome (`O.Ui` assembly; repo `o-ui`; **Hcw.Ui / hcw-ui retired**) | | `o-ui` | `O.Ui` |
| **O-Form**, **O-Dynamo** | Planned, not built | O-Form: Deep Sea Green `#075056` | `O-Form`, `O-Dynamo` | |

**Suite wire (implementation, not user-facing):** shared state lives under `%LOCALAPPDATA%\O-Studio\` (`suite.json`, `active-project.json`, `appearance.json`, `neon.env`, `selected-project.json`, `launch-request.json`, `ai\auth.token`). O-Studio starts sibling apps with `OSTUDIO_COMPANY`, one-time `OSTUDIO_TICKET`, optional `OSTUDIO_USER`, the locked project (`OSTUDIO_PROJECT_ID`, `OSTUDIO_PROJECT_OSP`), and Olmo client env (`OSTUDIO_AI_ROLE=client`, `OSTUDIO_AI_HOST`, `OSTUDIO_AI_PORT`, `OSTUDIO_AI_TOKEN`). Project file meta format id is **`osp`** v1; the active-project pointer uses **`studio-active-project`** v1. Constants: `Office.Shared.SuiteWire` (.NET), `Osas.Core.SuiteWire` (O-Studio), `aad-office::wire` (O-Draft).

Other names:

| Term | Meaning |
|---|---|
| **Company file** | One file per practice, `.osas` (earlier `.adms` opens too). SQLite, optionally encrypted (`AOSENC1`). Chosen and created only in **O-Studio**; opened read-write by O-Studio, read-only for details by O-CoreOp and O-Draft |
| **Project file** | `.osp`. One SQLite file per project holding the drawing, the take-off and the project record. Format name `osp`, version 1. (`.odp` was the working name and clashed with LibreOffice Impress; it is not used) |
| **Take-off** | Quantities measured from a drawing. Recorded by O-Draft into the project file; read by O-CoreOp |
| **Revision** | The counter inside a project file (`meta.revision`). Every save by any program is one revision. Apps compare revisions to know someone else wrote |
| **Active-project pointer** | `%LOCALAPPDATA%\O-Studio\active-project.json`, written by O-Draft: which project file is open there, so O-CoreOp can open the same one |
| **Locked project** | The project chosen in the **launcher** (or O-Studio) for this PC: `%LOCALAPPDATA%\O-Studio\selected-project.json` (company, project id, code, name, project file, who locked it). O-CoreOp and O-Draft open it directly and have no New, Open or Save as project. It cannot be changed while O-CoreOp or O-Draft is running |
| **Ticket** | One-time sign-in token **O-Studio** gives an app it starts: 32 random bytes, two minutes, one use. The password is never passed on (`OSTUDIO_TICKET` in the environment) |
| **Persona** | In O-CoreOp, Project Manager/PMC or Contractor: each has its own letterhead and numbering |
| **AI Core** | Loopback assistant service on `127.0.0.1:8742` (`AiCoreListener`). the **launcher** hosts **Olmo 3 7B** for as long as it runs; **O-Studio**, **O-CoreOp** and **O-Draft** are clients only |
| **Olmo 3 7B** | The shared local assistant model (GGUF in `%LOCALAPPDATA%\HumanCentricWorks\models\`). Not Tulu and not in-process in O-CoreOp or O-Draft |

## 2. Hierarchy

```
O-Studio (company, users, practice records, Neon sync settings); the launcher (sign-in, project, AI Core host, cloud sync)   <- which company, who is signed in
 |
 +-- Company file (.osas)                                 <- the practice: clients, projects, users, accounts
 |     |
 |     +-- Project (ProjectRef: company_uuid + project_id)  <- identity shared by all apps
 |           |
 |           +-- Project file (.osp)                        <- the working data of one project
 |                 |-- drawing(s)  (DWG/DXF bytes, unchanged)
 |                 |-- take-off    (headers, rows, annotated PDFs)
 |                 |-- project record (estimate, bills, tasks, quantities, POs)
 |                 |-- markups, history, change log
 |
 +-- O-CoreOp   reads company details; reads/writes the project record and take-off view in the .osp
 +-- O-Draft    writes the drawing and take-off in the .osp
 +-- Viewer (planned: tablet/phone)  views and marks up the synced project
```

Rule: **a project's data lives in its `.osp`; the practice's data lives in the company file; nothing else is a source of truth.** Cloud (**Neon** — Lakebase Postgres and Object Storage) is a master copy that O-Studio syncs; the apps never hold cloud keys.

## 3. Roles

Users belong to the company, so they are the same in every app. Roles are in `office-core` (`auth.rs`):

| Role | Can |
|---|---|
| **Owner** | Everything: settings, users, backup, post, close periods, issue, view audit |
| **Principal** | View, issue |
| **Accounts** | View, issue, post |
| **Document editor** | View and edit documents |
| **Auditor** | View, and view the audit trail (read only) |

Users are added, re-roled and re-passworded in **O-Studio** (Office > Users or company sign-in flows), each change asking for the owner's password again; the last active owner cannot be removed. In O-CoreOp the company file is read-only details (name, address, GSTIN, letterhead); the project's persona (PM/PMC or Contractor) is separate from the user's role.

## 4. What each app has on screen

### O-Draft (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Menu bar | File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Architecture, Window, Help |
| Top tool strip (row 1) | Layer box, **Draw / Modify / Annotate** (horizontal), architecture tools, gear (View > Customize Top Bar) |
| Take-off strip (row 2) | **Take-off tools only** (Ext wall, Int wall, Beam, … Record, Map layers, phase pickers) while **Take-off** is on in Classic workspace |
| Right | Layers / Properties / Blocks tabbed stack |
| Bottom bar | Snap/ortho/polar toggles, coordinates, units, **Take-off toggle** (next to Nameplate) |
| File | Save, Close, Export, Print/Plot, History. No New, Open or Save as: the project is chosen and locked in the **launcher**, and O-Draft opens its project file (made on first open). Drawing setup (floors, layer sets, tabs) stays in O-Draft |
| Command line | Every tool is also a typed command, e.g. `takeoff.record` |

Details: `docs/UI_UX.md`.

### O-CoreOp (WinUI shell, C++ engine)

| Region | Contents |
|---|---|
| Main ribbon | One text tab per section: Project, RCC steel, Civil BOQ, Schedule, Contracts, Stores, Outputs (Ctrl+1 to Ctrl+7). Office, Accounts and HR live in O-Studio |
| Sub ribbon | One text tab per page of the open section |
| Take-off page | Reads the take-off from the open `.osp`; shows "O-Draft updated the take-off" when O-Draft writes; Import brings it into the estimate |
| File | Save, Take-off, Import from CAD, Export. No New, Open or Save as project: the project is chosen and locked in the **launcher**, and O-CoreOp opens it. No company menu: the company comes from **O-Studio** (launch from the launcher or Studio, which redeems a ticket) |
| Bottom shell | Full-bleed status row + floating Ask pill (`O.Ui` `SuiteBottomStatusBar`, `SuiteAskBar`) — O-Draft layout is canonical |
| Assistant (Olmo) | Hosted by the launcher on `:8742`; O-CoreOp and O-Draft are clients |

Details: `docs/WINUI-SHELL.md`.

### O-Studio (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Sign-in / company | New or open company file (`.osas`); sign in once; remember path in `suite.json`; admin password and **Users** |
| Left pane | Practice (Pulse, Clients, Projects, enquiries, site records, permits, proposals, meetings); Documents (letters, templates); Billing (invoices, receipts, reconciliation); Books (expenses, income, salaries, assets, journals); Reports and tax; Office (firm profile, staff, leave, attendance, library, security, backup); **Suite** (dashboard, **Cloud sync**, approvals) |
| Shortcuts | Ctrl+N new record, Ctrl+S save, Ctrl+P print, Ctrl+F search, Ctrl+K go to page |
| Bottom shell | Same status row and Ask pill as O-Draft and O-CoreOp (`O.Ui`); **launch O-CoreOp, O-Draft, O-Form** from the status row |
| Assistant (Olmo) | Client of the launcher's host on `:8742`; read-only practice tools in the floating bar |

## 5. How they are linked, and how it works live

| Link | Mechanism | Latency |
|---|---|---|
| O-Studio -> apps | Starts the app with `OSTUDIO_COMPANY` + one-time `OSTUDIO_TICKET` (and Olmo client env when applicable) | on launch |
| Company -> apps | `office.dll` (C ABI from `office-ffi`), read by the app | on open |
| O-Draft -> O-CoreOp | Both open the **same `.osp`**. O-Draft's `takeoff.record` writes headers/rows/annotated PDF in one revision; O-CoreOp polls `meta.revision` every 2 s and notifies | about 2 s, no server |
| Launcher -> apps (which project) | The launcher locks one project (`selected-project.json`) and starts O-CoreOp and O-Draft with `OSTUDIO_PROJECT_ID` + `OSTUDIO_PROJECT_OSP`; an app started alone, or a double-clicked `.osp`, asks the launcher (`O-Studio.Launcher.exe --launch <app>` / `--open <file>`), which signs in, locks the project and starts the app | on launch |
| O-Draft -> O-CoreOp (which file) | Active-project pointer (kept for one release; the lock decides now): `{format:"studio-active-project", app, drawing, project, updated_utc}` | on drawing change |
| Two programs, one file | SQLite WAL, immediate write transactions, 5 s busy timeout: writers take turns, every commit is exactly one revision, readers never block (tested with 4 writers x 40 commits) | immediate |
| Machine -> machine | O-Studio syncs `.osp` with **Neon** by revision (connector MVP; see `docs/CLOUD-NEON.md`); a viewer on tablet/phone reads what O-Studio pushed and sends markups back | Postgres revision + Object Storage |

Both apps share one format library: Rust `aad-odp` (O-Draft) and .NET `Office.Shared.OdpFile` (O-CoreOp), kept compatible by tests that read each other's files.

Saving: O-Draft writes the drawing back into the `.osp` only when its bytes changed (so a take-off-only change does not touch the drawing), and records take-off changes as differences only.

## 6. Where to read next

| Topic | Document |
|---|---|
| Project file format and sync plan | O-Studio `docs/ODP-PROJECT-FILE-AND-SYNC.md` (filename keeps the working name) |
| Project hierarchy and security | `docs/PROJECT-HIERARCHY-AND-SECURITY.md` |
| Suite architecture and integration map | O-Studio `docs/suite-architecture.md`, `docs/suite-hub.md` |
| Suite hub and wire | O-Studio `docs/suite-hub.md`, `Office.Shared/SuiteWire.cs` |
| Company file ABI | O-Studio `docs/office-abi.md` |
| O-Draft UI, commands, performance | O-Draft `docs/UI_UX.md`, `docs/PERFORMANCE.md`, `docs/PERFORMANCE-AUDIT-2026-10-30.md` |
| O-CoreOp shell, model, take-off | O-CoreOp `docs/WINUI-SHELL.md`, `docs/LOCAL-MODEL.md`, `docs/PROJECT-FILE.md` |
| O-Studio assistant and Neon sync | O-Studio `docs/AI_RUNTIME.md`, `docs/CLOUD-NEON.md`, `docs/ODP-PROJECT-FILE-AND-SYNC.md` |
| Shared WinUI chrome | O-UI (`O.Ui`) — `SuiteBottomStatusBar`, `SuiteAskBar`, theme tokens |
| ShilpiDB → `.osp` migration (historical) | Each app repo `docs/MIGRATION-OSP.md` — do not use **ShilpiDB** in new user text |
| Installers | O-Studio `installer/suite/README.md`; O-CoreOp `docs/PACKAGING.md`, `docs/SUITE-PACKAGING.md` |
| Testing | O-Studio `docs/testing/README.md` (plans for every app, the connected apps, bug process and log) |
| Brand (palette, the mark, the AI agent orb, type) | O-Studio `docs/BRANDING.md`; artwork and colour files in `o-ui` (`brand/`, `Themes/SuiteAccent.*.xaml`) |

## 7. Conventions

- **App names (user-visible):** O-Studio, O-CoreOp, O-Draft, O-UI. Spell exactly; do not shorten to Studio, CoreOp, or Draft in menus, dialogs, or docs.
- **Suite hub:** say **O-Studio** — not OSAS, not a separate launcher app, not "the OSAS suite" in user text.
- **Never in user text:** AQC, AOS, AAD, AADT, ShilpiDB, Tulu, **OSAS** as a product name, **OSAS.exe** (assistant is **Olmo 3 7B**). Legacy exe names (`AQCCore.exe`, `aad-host.dll`) belong in migration or identifier columns only.
- **Files:** project file = **`.osp`**; company file = **`.osas`**. Do not call the project file ODP or `.odp` in user text.
- **Take-off:** hyphenated **Take-off** (not takeoff or take off) in UI and documentation.
- **Company file** vs **project file:** practice-wide data lives in the company file; one project's drawing, take-off, and estimate live in its `.osp`.
- New project data goes in the `.osp` schema with a change-log row, not a sidecar file.
- Apps do not call each other; they meet in the `.osp`, the company file, the active-project pointer, or the AI Core on loopback.
- Cloud keys, Neon credentials, and the Olmo host process live only in the **launcher** and **O-Studio**; O-CoreOp and O-Draft hold none.
