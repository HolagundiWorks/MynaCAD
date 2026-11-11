# The Myna suite

**Myna** is the name of the suite, a family of Windows apps for architecture and construction practices. Each app is Myna plus a use-case word. There is no app called Myna.

| App | What it is | Repository | Status |
|---|---|---|---|
| **MynaHub** | Company hub: company file, sign-in, users and roles, practice records, estimates and bills, approvals, the only cloud sync. No AI | `HolagundiWorks/MynaHub` (private) | Shipped |
| **MynaOps** | Quantities, estimates, rate books, bar bending, BOQ, purchase orders, running bills, scheduling. Hosts the local AI agent and the project database manager | `HolagundiWorks/MynaOPS` | Shipped |
| **MynaCAD** | 2D drafting: DXF/DWG, levels, layouts, plotting, architecture and take-off tools. A client of the AI agent | `HolagundiWorks/MynaCAD` | Shipped |
| **MynaUI** | Shared WinUI 3 library (assembly `O.Ui`): themes, controls, marks | `HolagundiWorks/MynaUI` | Library |
| **Myna3D** | 3D / BIM modelling, a Blender 5.2.3 LTS clone (was MynaMod, earlier Finch) | `HolagundiWorks/o-form` | Planned, not built |

## How the apps connect

- **MynaCAD and MynaOps are installed together and share one local `.osp` project file per project** (SQLite). MynaCAD writes the take-off into it; MynaOps reads it. Changes to the `.osp` format are additive only (new meta keys or tables).
- **MynaHub is independent of both.** It starts them by path and passes nothing: no sign-in, no ticket, no project. Project data reaches the company database only through MynaOps's CSV export, imported in MynaHub.
- **The AI agent** (Olmo 3 7B through `llama-server`, loopback `127.0.0.1:8742`) is hosted by MynaOps, in a window or headless (`O-CoreOp.exe --ai-host`), and serves MynaOps and MynaCAD. MynaCAD starts the host when none answers.
- **Cloud sync (Neon)** belongs to MynaHub only. MynaCAD and MynaOps never hold cloud keys. Neon work is paused for now.

## Installers

Two installers: `MynaHub-Setup-<version>.exe` and `MynaOpsCAD-Setup-<version>.exe` (MynaOps and MynaCAD together). They are built by `installer/suite/build-all.ps1` in the MynaHub repository from the working trees of MynaHub, MynaOps and MynaCAD.

## Folder layout for development

The repositories must sit side by side because project files reference each other by relative path:

```
Repos\MynaCAD   Repos\MynaOps   Repos\MynaHub   Repos\MynaUI   (Repos\Myna3D)
```

## Names and old spellings

User-visible text uses the Myna names. Identifiers keep their old spellings on purpose, so upgrades and existing files keep working: install folders `O-Studio`, `O-CoreOp`, `O-Draft`; executables `O-Studio.exe`, `O-CoreOp.exe`, `ODraft.WinUi.exe`; `OSTUDIO_*` environment variables; `%LOCALAPPDATA%\O-Studio` and `\OSAS`; the `.osas` and `.bbsproj` extensions; the stored drawing stamp `"app":"Tren"`.

The authority is `docs/SUITE.md`, which has an identical copy in MynaHub, MynaOps and MynaCAD.
