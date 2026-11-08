# Kivo suite: names, roles, files, links and how it works live

**Canonical copy:** the Kivo repository, `docs/SUITE.md`. Each app repository carries an identical copy at `docs/SUITE.md` so a developer in any one repo knows what the others are, what they are called, what their toolbars hold and how they connect. If you change one, change all four. Updated 2026-11-07.

## 1. Names

The suite is the **Kivo family of apps** (Kivo, Falcon, Tren, Finch, Myna, and O-UI). **Kivo** is the hub: **company sign-in** (the session every other app asks for), users, estimates, running bills, **approvals and revisions**, and the Neon sync settings. **Myna** hosts only the **Olmo AI Core** and the **database service** (a local database that syncs to Neon); it does not sign anyone in. **Tren** and **Falcon** share one `.osp` file connected to that database. A name below is the name to use in text, menus and installers. Code identifiers keep older spellings because files already use them; they are identifiers, not names.

| Name to use | What it is | Colour | Repository / folder | Identifiers that keep old spellings |
|---|---|---|---|---|
| **Kivo** | Office and projects; **suite hub** (company file, users, launch Falcon/Tren, approvals; the one app that is synced to the cloud) | Ember `#B83E16` | `O-STUDIO` | `aos`, `AOSENC1`, `office-core`, `office.dll` |
| **Falcon** | Quantities, estimates, rate books, bar bending, BOQ, purchase orders, running bills, scheduling | Dusk `#5630A0` | `O-COREOP` | `AQCCore.exe` (old), `bbs_engine.dll`, `.bbsproj` (old project files), `aqc` |
| **Tren** | 2D CAD: drafting, DXF/DWG, layouts, plotting, architecture tools and the take-off tools | Midnight `#102C80` | `O-DRAFT` | `aad-*` crates, `aad-host.dll`, `aad-winui` |
| **O-UI** | Shared WinUI themes, marks and chrome (`O.Ui` assembly; repo `o-ui`; **Hcw.Ui / hcw-ui retired**) | | `o-ui` | `O.Ui` |
| **Finch**, **O-Dynamo** | Planned, not built | Finch: Grove `#166040` | `Finch`, `O-Dynamo` | |

**Suite wire (implementation, not user-facing):** shared state lives under `%LOCALAPPDATA%\O-Studio\` (`suite.json`, `active-project.json`, `appearance.json`, `neon.env`, `selected-project.json`, `launch-request.json`, `ai\auth.token`). Kivo starts sibling apps with `OSTUDIO_COMPANY`, one-time `OSTUDIO_TICKET`, optional `OSTUDIO_USER`, the locked project (`OSTUDIO_PROJECT_ID`, `OSTUDIO_PROJECT_OSP`, `OSTUDIO_PROJECT_NAME`), and Olmo client env (`OSTUDIO_AI_ROLE=client`, `OSTUDIO_AI_HOST`, `OSTUDIO_AI_PORT`, `OSTUDIO_AI_TOKEN`). Project file meta format id is **`osp`** v1; the active-project pointer uses **`studio-active-project`** v1. Constants: `Office.Shared.SuiteWire` (.NET), `Osas.Core.SuiteWire` (Kivo), `aad-office::wire` (Tren).

Other names:

| Term | Meaning |
|---|---|
| **Company file** | One file per practice, `.osas` (earlier `.adms` opens too). SQLite, optionally encrypted (`AOSENC1`). Chosen and created in **Kivo**. **Falcon** signs in as a Kivo user and pushes project items into it. **Tren** does not open it |
| **Project file** | `.osp`. One SQLite file per project holding the drawing, the take-off and the project record. Format name `osp`, version 1. (`.odp` was the working name and clashed with LibreOffice Impress; it is not used) |
| **Take-off** | Quantities measured from a drawing. Recorded by Tren into the project file; read by Falcon |
| **Revision** | The counter inside a project file (`meta.revision`). Every save by any program is one revision. Apps compare revisions to know someone else wrote |
| **Active-project pointer** | `%LOCALAPPDATA%\O-Studio\active-project.json`, written by Tren: which project file is open there, so Falcon can open the same one |
| **Shared project file** | One `.osp` opened in **Tren** and **Falcon**. Double-click opens it in Tren; **Open in Falcon** is a separate verb. Neither verb starts Myna. Tren writes the drawing and take-off; Falcon writes quantities, the estimate, bills, and the project maps and layouts |
| **Ticket** | One-time sign-in token **Kivo** gives an app it starts: 32 random bytes, two minutes, one use. The password is never passed on (`OSTUDIO_TICKET` in the environment) |
| **Persona** | In Falcon, Project Manager/PMC or Contractor: each has its own letterhead and numbering |
| **AI Core** | Loopback assistant service on `127.0.0.1:8742` (`AiCoreListener`). **Myna** hosts **Olmo 3 7B** for as long as it runs; **Kivo**, **Falcon** and **Tren** are clients only |
| **Olmo 3 7B** | The shared local assistant model (GGUF in `%LOCALAPPDATA%\HumanCentricWorks\models\`). Not Tulu and not in-process in Falcon or Tren |

## 1a. The workflow (from suite 1.5.0)

**Built in 1.5.0:** Kivo's sign-in and session broker, the slimmed Myna with the database service, Falcon's CSV export and Kivo's CSV import. **Built after 1.5.5 (next suite build):** Tren and Falcon connect the open `.osp` to the database service (ask Kivo for a session, connect, push each saved revision; status in the status bar; Tren `docs/DB-CONNECT.md`). **Not built yet:** moving approvals and revisions fully into Kivo; pulling a newer shared revision into an open file.

Draw in **Tren**, measure in **Falcon**, both on **one `.osp` file** connected to the suite database. **Falcon exports a CSV; Kivo imports it** for estimates and running bills. **Approvals and revisions live in Kivo.**

| App | Owns | No longer owns |
|---|---|---|
| **Myna** | The **AI Core** (Olmo, loopback `127.0.0.1:8742`) and the **database service** (loopback `127.0.0.1:8743`, the local shared project database; project files are not synced). Tray icon. | Company sign-in |
| **Kivo** | **Company sign-in** and the session; estimates, running bills, **approvals, revisions**; **Import from Falcon (CSV)** | |
| **Tren** | The drawing and its take-off rows, written into the `.osp` | Sign-in, approvals, revisions |
| **Falcon** | Quantities, rates, the estimate; **Export for Kivo (CSV)** | Sign-in, approvals, revisions |

How the pieces connect:

- **Sign-in on demand.** When Tren or Falcon first needs the database it asks Kivo for a session over the local named pipe `\\.\pipe\osas-session-broker`. Kivo opens, the person signs in once, and a session token (32 random bytes, held in memory, 8 hours idle, 24 hours at most) is handed back. The apps never store company credentials. Contract: `contracts/session-broker.md` (Kivo repo, `docs/contracts/session-broker.md`).
- **Database.** Myna's database service checks every request's Bearer token with Kivo's broker. Project files stay on this PC, so the service reports state `local` and sends nothing; the local `.osp` is the shared database.
- **CSV.** Falcon writes `osas-falcon-csv/1` (20 columns; rows `meta`, `estimate`, `measured`, `takeoff`, `total`; UTF-8 with BOM, RFC 4180, CRLF). Kivo previews it and imports it in one transaction onto estimate versions and draft running bills. Contracts: `contracts/falcon-csv.md` (Kivo repo, `docs/contracts/falcon-csv.md`), `contracts/falcon-csv-import.md` (Kivo repo, `docs/contracts/falcon-csv-import.md`); a running bill locks its measured-to-date once certified.
- **Design note:** `WORKFLOW-KIVO-HUB.md` (Kivo repo, `docs/WORKFLOW-KIVO-HUB.md`).

## 2. Hierarchy

```
Kivo (company sign-in, users, practice records, estimates, bills, approvals, revisions); Myna (AI Core host, database service, Neon sync)   <- which company, who is signed in
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
 +-- Falcon   reads company details; reads/writes the project record and take-off view in the .osp
 +-- Tren    writes the drawing and take-off in the .osp
 +-- Viewer (planned: tablet/phone)  views and marks up the synced project
```

Rule: **a project's data lives in its `.osp`; the practice's data lives in the company file; nothing else is a source of truth.** Cloud (**Neon** — Lakebase Postgres and Object Storage) is a master copy that Kivo syncs; the apps never hold cloud keys.

## 3. Roles

Users belong to the company, so they are the same in every app. Roles are in `office-core` (`auth.rs`):

| Role | Can |
|---|---|
| **Owner** | Everything: settings, users, backup, post, close periods, issue, view audit |
| **Principal** | View, issue |
| **Accounts** | View, issue, post |
| **Document editor** | View and edit documents |
| **Auditor** | View, and view the audit trail (read only) |

Users are added, re-roled and re-passworded in **Kivo** (Office > Users or company sign-in flows), each change asking for the owner's password again; the last active owner cannot be removed. Falcon signs in as one of those users and pushes project items; it does not edit users. Firm details for Tren title blocks are an exported pack, not a live company-file session. The project's persona (PM/PMC or Contractor) is separate from the user's role.

## 4. What each app has on screen

### Tren (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Menu bar | File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Architecture, Window, Help |
| Top tool strip (row 1) | Layer box, **Draw / Modify / Annotate** (horizontal), architecture tools, gear (View > Customize Top Bar) |
| Take-off strip (row 2) | **Take-off tools only** (Ext wall, Int wall, Beam, … Record, Map layers, phase pickers) while **Take-off** is on in Classic workspace |
| Right | Layers / Properties / Blocks tabbed stack |
| Bottom bar | Snap/ortho/polar toggles, coordinates, units, **Take-off toggle** (next to Nameplate) |
| File | New, Open, Save, Save as, Close, Export, Print/Plot, History, and **Import company details** (`company.pack.import`). The project file is the `.osp` shared with Falcon. Company details for title blocks are a JSON pack exported from Kivo (Firm settings, **Export for Tren**). Tren does not call Kivo or Myna |
| Command line | Every tool is also a typed command, e.g. `takeoff.record` |

Details: `docs/UI_UX.md`.

### Falcon (WinUI shell, C++ engine)

| Region | Contents |
|---|---|
| Main ribbon | One text tab per section: Project, RCC steel, Civil BOQ, Schedule, Contracts, Stores, Outputs (Ctrl+1 to Ctrl+7). Office, Accounts and HR live in Kivo |
| Sub ribbon | One text tab per page of the open section |
| Take-off page | Reads the take-off from the open `.osp`; shows "Tren updated the take-off" when Tren writes; Import brings it into the estimate |
| Layout page | Outputs > Layout. The left side is the project map and its layouts (for example Ground floor working drawing). The right side lists print blocks saved in the open `.osp` by Tren. Insert places the related print block, Update nameplate stores the title fields in the project document, and Export PDF writes a sheet record. The PDF does not redraw the model |
| File | Open project (the shared `.osp`), Save, **Sign in to Kivo**, **Push to Kivo**, Take-off, Import from CAD, Export. Sign-in uses the Kivo user and the company file. Push writes this project's items (`aqc.project`, and an estimate draft when one has been calculated) |
| Bottom shell | Full-bleed status row + floating Ask pill (`O.Ui` `SuiteBottomStatusBar`, `SuiteAskBar`) — Tren layout is canonical |
| Assistant (Olmo) | Hosted by Myna on `:8742`; Falcon and Tren are clients |

Details: `docs/WINUI-SHELL.md`.

### Kivo (WinUI shell over a Rust engine)

| Region | Contents |
|---|---|
| Sign-in / company | New or open company file (`.osas`) **in Kivo**; sign in once (Kivo serves the session to Falcon and Tren on request); remember path in `suite.json`; admin password and **Users** (in Kivo) |
| Left pane | Practice (Pulse, Clients, Projects, enquiries, site records, permits, proposals, meetings); Documents (letters, templates); Billing (invoices, receipts, reconciliation); Books (expenses, income, salaries, assets, journals); Reports and tax; Office (firm profile, staff, leave, attendance, library, security, backup); **Suite** (dashboard, **Cloud sync**, approvals) |
| Shortcuts | Ctrl+N new record, Ctrl+S save, Ctrl+P print, Ctrl+F search, Ctrl+K go to page |
| Bottom shell | Same status row and Ask pill as Tren and Falcon (`O.Ui`); **launch Falcon, Tren, Finch** from the status row |
| Assistant (Olmo) | Client of Myna's host on `:8742`; read-only practice tools in the floating bar |

## 5. How they are linked, and how it works live

| Link | Mechanism | Latency |
|---|---|---|
| Kivo -> apps | Starts the app with `OSTUDIO_COMPANY` + one-time `OSTUDIO_TICKET` (and Olmo client env when applicable) | on launch |
| Company -> apps | `office.dll` (C ABI from `office-ffi`), read by the app | on open |
| Tren -> Falcon | Both open the **same `.osp`**. Tren's `takeoff.record` writes headers/rows/annotated PDF in one revision; Falcon polls `meta.revision` every 2 s and notifies | about 2 s, no server |
| Falcon -> company file | File > **Sign in to Kivo** (`auth.login` on the `.osas`), then **Push to Kivo**. If the open `.osp` is not linked, Falcon asks which Kivo project receives the items | on push |
| Kivo -> Tren (company details) | Firm settings > **Export for Tren** writes `osas-company-pack` v1 JSON (name, address, phone, email, PAN, GSTIN, COA registration, architect). Tren **Import company details** applies it to title blocks. No live session and no launcher | on import |
| Tren -> Falcon (which file) | The same `.osp`, opened in each app. Active-project pointer remains: `{format:"studio-active-project", app, drawing, project, updated_utc}` | on drawing change |
| Two programs, one file | SQLite WAL, immediate write transactions, 5 s busy timeout: writers take turns, every commit is exactly one revision, readers never block (tested with 4 writers x 40 commits) | immediate |
| Machine -> machine | Kivo syncs `.osp` with **Neon** by revision (connector MVP; see `docs/CLOUD-NEON.md`); a viewer on tablet/phone reads what Kivo pushed and sends markups back | Postgres revision + Object Storage |

Both apps share one format library: Rust `aad-odp` (Tren) and .NET `Office.Shared.OdpFile` (Falcon), kept compatible by tests that read each other's files.

Saving: Tren writes the drawing back into the `.osp` only when its bytes changed (so a take-off-only change does not touch the drawing), and records take-off changes as differences only.

## 6. Where to read next

| Topic | Document |
|---|---|
| Project file format and sync plan | Kivo `docs/ODP-PROJECT-FILE-AND-SYNC.md` (filename keeps the working name) |
| Project hierarchy and security | `docs/PROJECT-HIERARCHY-AND-SECURITY.md` |
| Suite architecture and integration map | Kivo `docs/suite-architecture.md`, `docs/suite-hub.md` |
| Suite hub and wire | Kivo `docs/suite-hub.md`, `Office.Shared/SuiteWire.cs` |
| Company file ABI | Kivo `docs/office-abi.md` |
| Tren UI, commands, performance | Tren `docs/UI_UX.md`, `docs/PERFORMANCE.md`, `docs/PERFORMANCE-AUDIT-2026-10-30.md` |
| Falcon shell, model, take-off | Falcon `docs/WINUI-SHELL.md`, `docs/LOCAL-MODEL.md`, `docs/PROJECT-FILE.md` |
| Kivo assistant and Neon sync | Kivo `docs/AI_RUNTIME.md`, `docs/CLOUD-NEON.md`, `docs/ODP-PROJECT-FILE-AND-SYNC.md` |
| Shared WinUI chrome | O-UI (`O.Ui`) — `SuiteBottomStatusBar`, `SuiteAskBar`, theme tokens |
| ShilpiDB → `.osp` migration (historical) | Each app repo `docs/MIGRATION-OSP.md` — do not use **ShilpiDB** in new user text |
| Installers | **Unified:** Kivo `installer/suite/README.md` (`Kivo-Suite-Setup-<version>.exe`, install and remove modes). Per-app: Kivo `docs/packaging.md`, Falcon `docs/PACKAGING.md`, Tren `docs/RELEASE_AND_PACKAGING.md` |
| Testing | Kivo `docs/testing/README.md` (plans for every app, the connected apps, bug process and log) |
| Brand (palette, the mark, the AI agent orb, type) | Kivo `docs/BRANDING.md`; artwork and colour files in `o-ui` (`brand/`, `Themes/SuiteAccent.*.xaml`) |

## 7. Conventions

- **App names (user-visible):** Kivo, Falcon, Tren, Finch, Myna, and O-UI. Spell exactly. Finch is planned. Myna is the launcher. Do not use the earlier names O-Studio, O-CoreOp, O-Draft, or O-Form in menus, dialogs, or user-facing sentences. Those names remain ticket ids, install folders, and executable names.
- **Suite hub:** say **Kivo**. The launcher is **Myna**. Do not say OSAS or "the OSAS suite" in user text.
- **Never in user text:** AQC, AOS, AAD, AADT, ShilpiDB, Tulu, **OSAS** as a product name, **OSAS.exe** (assistant is **Olmo 3 7B**). Legacy exe names (`AQCCore.exe`, `aad-host.dll`) belong in migration or identifier columns only.
- **Files:** project file = **`.osp`**; company file = **`.osas`**. Do not call the project file ODP or `.odp` in user text.
- **Take-off:** hyphenated **Take-off** (not takeoff or take off) in UI and documentation.
- **Company file** vs **project file:** practice-wide data lives in the company file; one project's drawing, take-off, and estimate live in its `.osp`.
- New project data goes in the `.osp` schema with a change-log row, not a sidecar file.
- Apps do not call each other; they meet in the `.osp`, the company file, the active-project pointer, or the AI Core on loopback.
- Cloud keys, Neon credentials, and the Olmo host process live only in **Myna** and **Kivo**; Falcon and Tren hold none.

## AI agent privacy

The AI agent (Olmo 3 7B) runs on this computer and is hosted by Myna. Myna's Ask page has a **Privacy** panel whose choices are saved in `%LOCALAPPDATA%\O-Studio\ai-privacy.json` (`Osas.Core.Suite.AiPrivacy`): tell it which project is loaded (default off), remember the conversation while the window is open (default off), allow attaching files with the + button (default on), and Clear this conversation. Not done yet: a switch to stop the agent altogether, per-app permissions for Tren and Falcon, and the same panel in those apps' prompt bars.
