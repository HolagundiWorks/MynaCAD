# Migration: ShilpiDB / `.vdb` → project file (`.osp`)

**Status date:** 2026-11-04 (suite installer flows). **Canonical suite terms:** [SUITE.md](SUITE.md). **Format spec:** MynaHub `docs/ODP-PROJECT-FILE-AND-SYNC.md`. **MynaOps behaviour:** [PROJECT-FILE.md](PROJECT-FILE.md).

This page is the living checklist for retiring the old take-off store (ShilpiDB, `shilpid`, sidecar `.vdb` files) and standardising on **one `.osp` per project**. Keep a copy of this file in sync in each app repo (`docs/MIGRATION-OSP.md`) when you change it.

## What changed (product)

| Before | Now |
|---|---|
| Take-off in a `.vdb` beside the drawing or on a shared `shilpid` server | Take-off in SQLite tables inside the **`.osp`** |
| MynaOps opened ShilpiDB projects / `.vdb` / gateway | MynaOps opens **`.osp`** only; Take-off reads `takeoff_*` via `OdpTakeoffAdapter` |
| Separate `.bbsproj` for estimate data | **`project_doc`** inside the same `.osp` |
| Wire codename "ShilpiDB" in docs and UI | **`.osp` project file**; code types `OspTakeoff`, `OdpTakeoffService` |

**Unchanged on purpose (wire contract):**

- JSON header field `"format": "hcw-takeoff-shilpi"` in take-off headers (MynaCAD and MynaOps constants `HEADER_FORMAT_WIRE` / `HeaderFormatWire`).
- Bridge file `hcw-aqc-bridge` v1 and row JSON shape.
- Import idempotence prefix `source = "hcwcad:<drawing_id>"` (stored in user projects; not renamed without an owner migration).

**No automatic migration:** Legacy `.vdb` take-off databases and old ShilpiDB project files are **not** imported into `.osp` in shipping apps today. Users create or save as `.osp` and re-record take-off in MynaCAD if needed.

---

## Done (by area)

### MynaOps

- Take-off reader: `AQC.CadImport/OspTakeoff.cs`, tests `OspTakeoffTests.cs`.
- Project file I/O: `Office.Shared/OdpFile.cs`, `BBSApp/Services/Odp/*`, `ProjectFiles.cs`, `TakeoffPage.cs`.
- ShilpiDB connectors, gateway client, and duplicate `Services/Shilpi/` removed (re-delete if they reappear from an old branch).
- Docs: `PROJECT-FILE.md`, `SUITE.md`, `HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md`, `CLAUDE.md`, `CHANGELOG` pointer, `HANDOFF-O-COREOP.md`.
- **UIA e2e:** `tests/e2e/uia-osp-takeoff.ps1`, `run-e2e.ps1` (9 checks on `Office.Shared.Tests/Fixtures/small.osp`).
- **`dotnet test`:** `AQC.CadImport.Tests` (88), `Office.Shared.Tests` (91) — run before release.

### MynaCAD

- `takeoff.record` → open `.osp` (`crates/odp`, `takeoffrec.rs`).
- ShilpiDB commands, `crates/vdb`, installer bundling of `shilpid` removed.
- Docs updated; **screen inventory** pruned (ShilpiDB / `vdb.*` rows removed).
- Stale QA logs with `shilpidb` compile output removed from `docs/qa/artifacts/`.
- WinUI: `.osp` in File open/save; take-off toggle; no Shilpi status chip.

### MynaHub

- ShilpiDB panels, connectors, migration `0036_project_shilpi` removed; office-core schema at **35** migrations.
- **`office-core` `cargo test`:** 82 passed.
- Suite docs trimmed; **`src-tauri/Cargo.lock`** without `shilpi-*` crates.

### Shared

- **`docs/SUITE.md`** (four repos): names, `.osp` hierarchy, active-project pointer, revision polling.

---

## Still open (owner / environment)

| Item | Notes |
|---|---|
| **`.vdb` / `.bbsproj` → `.osp` importer** | Spec in MynaHub ODP doc; `aad-odp` has take-off read/write, not a full legacy importer in apps |
| **Neon sync** | MynaHub planned work |
| **Draft WinUI → record → CoreOp import** on a **new** `.osp` | Product E2E beyond fixture-based UIA |
| **Scanned PDF underlay polish** | MynaCAD trust work |
| **`bbs_engine.dll` on dev machines without MSVC** | Install VS Build Tools; `cmake` in `BBSDesktop` |
| **MynaHub dev DBs at `user_version = 36`** | `PRAGMA user_version = 35` or recreate after dropping migration 36 |

### Explicitly not planned

- Renaming wire format `hcw-takeoff-shilpi` or import tag `hcwcad:` without owner migration.
- Re-adding ShilpiDB server or `.vdb` sidecars for take-off.

---

## Where to read (current)

| Topic | Document |
|---|---|
| Suite names and links | `docs/SUITE.md` |
| MynaOps and `.osp` | `docs/PROJECT-FILE.md` |
| Format + sync roadmap | MynaHub `docs/ODP-PROJECT-FILE-AND-SYNC.md` |
| MynaCAD project file | MynaCAD `docs/PROJECT-FILE.md` |
| Bridge / CAD import | `docs/bridge/`, `docs/AADT.md` |
| Parametric walls hand-off | `docs/HANDOFF-O-COREOP-PARAMETRIC-TAKEOFF.md` |
| Record layout | `AQC.CadImport/OspTakeoff.cs`, `docs/bridge/`, fixtures `odraft-takeoff-records.json` |
| Manual UIA e2e | MynaOps `BBSDesktop/tests/e2e/README.md` |

---

## Agent checklist

When touching take-off or project files:

1. Read MynaOps `docs/AADT.md` and `docs/PROJECT-FILE.md`.
2. Do not reference ShilpiDB, `shilpid`, or `VdbFile` in new code or user-facing text.
3. Use `OspTakeoff` / `OdpTakeoffAdapter` and `Services/Odp/` in MynaOps.
4. If you change this checklist, update **`docs/MIGRATION-OSP.md` in all four suite repos** (MynaHub, MynaOps, MynaCAD, MynaHub repo).
