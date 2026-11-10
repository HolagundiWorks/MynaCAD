# Known issues

Current state only. Detail, evidence and history are in [BUG_BACKLOG.md](BUG_BACKLOG.md) and
[UI_BUG_BACKLOG.md](UI_BUG_BACKLOG.md). Severity: P0 data loss or crash, P1 core drafting blocked,
P2 significant, P3 minor, P4 cosmetic. Confirmed means reproduced; a hypothesis has not been.

## Confirmed and open

| ID | Sev | Summary | Reproduce | Workaround |
|---|---|---|---|---|
| BUG-001 residual | P3 | In-process DWG reads (no `aad_dwg::isolated::enable()`) are still uncapped. The apps use the worker with a Windows Job Object memory cap (~2 GiB). The historical alloc-bomb fixture no longer aborts under `acadrust` 0.6.8 (opens empty); other unchecked AC18 shapes may still exist | Hostile DWG without isolation | Use the apps / isolated worker |
| UI-LINT debt | P3 | `GraphWindow` / `HistoryWindow` / `RevisionsWindow` / `WelcomeBusy` / `MainWindow.xaml` are at zero hard-coded dimensions (Density / Themes tokens). Remaining `LITERAL_BASELINE` entries are older manager/shell files plus `MainWindow.xaml.cs` (24) and `SmartBlockWindow.cs` (4); only lower a count when that file is cleaned ([UI_STANDARDS.md](UI_STANDARDS.md)) | `cargo xtask ui --counts` | Prefer tokens over new literals |
| (limit) | P1 | MynaCAD cannot write some object types to a DXF or DWG file yet (MULTILEADER, MLINE, 3DSOLID, REGION, and other objects read from another program's file and kept as `Unknown`): a save leaves them out of the file. The open drawing keeps them. Seen on a 26 206-entity working drawing: 164 MULTILEADER (133 in model space, listed as "Proxy" by `inspect` and `qselect.info`, which label every unmodelled object that way). The save says which: the command history (`Not saved ...`), `notSaved` in the save result and, since the shell shows it, a dialog after a save from the menu or picker ("Saved, but not everything was written"), through `Host::save_notice` / `aad_host_save_notice`. | Open such a file, save it as another name (or to `.osp`) and compare; `aad-cli convert` prints the same list | Keep the original file; edit a copy |
| (limit) | P3 | Command-line `open`/`qsave`, batch plot and PDF preview still run their commit/plot work on the session thread. Menu/picker and control-channel `app.open`/`app.save` use workers (control replies are deferred across `pump`) | Batch plot a large set | Wait |
| (limit) | P3 | A full display-list rebuild of every entity at zoom extents is still about 125 ms at 100 000 entities (post-rayon). Far-zoom LOD, frustum cull when zoomed in, and async band rebuild reduce freezes; they do not change full-extents O(n) ([PERFORMANCE.md](PERFORMANCE.md)) | Open or heavily edit a 100k drawing at extents | none |
| (limit) | P3 | Reading a large DWG takes about 1.2 to 1.5 s in the worker (acadrust + DXF round-trip) before the drawing appears; set `AAD_DWG_STAGE=1` for spawn timing. The window stays responsive in the WinUI shell | Open a large DWG | Wait |

## Fixed

BUG-001 (a damaged DWG aborted the program; Windows worker memory is now Job-capped), BUG-002 (stale
MSPACE viewport), UI-001 (stall on the first pick after opening a large drawing), PERF-001 (editing
1 000 objects took 40 ms instead of 5), layout viewport coarse-pan cache (8× cell + LRU), hatch/text
LOD, WinUI async zoom-band rebuild, control-channel and menu save via workers.

## Hypotheses (not reproduced)

- UI-003: file open and save block the UI thread — largely addressed for WinUI picker and control `app.open`/`app.save`; remaining session-thread cases are listed under Confirmed limits above.
- UI-004: `Ctrl+O` is declared by two commands; no misbehaviour seen.
- UI-005: an overly wide Properties panel in a layout tab, seen once, not reproduced.
- GitHub: `cad-ci.yml` runs on every push (it passed on 2026-10-27 and failed at the `fmt` step of a later push, for formatting in the companion code); `cad-nightly.yml` failed on its last three scheduled runs (2026-10-25 to 2026-10-27) and the cause has not been looked into.
- The release installer was installed, run and uninstalled silently on the development machine, but not on a clean machine, and the wizard pages were not exercised ([RELEASE_AND_PACKAGING.md](RELEASE_AND_PACKAGING.md)).
- Packages are often unsigned until release signing secrets are set (SmartScreen warns). Staging
  copies `NOTICE`, `ATTRIBUTION.md` and `THIRD_PARTY_LICENSES.md` into the MSI and portable zip
  (`packaging/windows/package.ps1`, `aad.wxs`); re-verify on the next release candidate.
- Dependencies are not audited for advisories on this machine (`cargo audit` is run by the nightly
  workflow only).

## Not implemented (by design for now)

Anything listed under "Planned" in [status.md](status.md): macOS, Linux and web builds, the local
language-model runtime in the apps, the remaining hcwCAD-KIT tools, the floor-project wizard,
user coordinate systems, SHX fonts.

## Layer protection and drawing spaces (added with the context bar)

- **Confirmed gap:** objects inside **block definitions** are not covered by the lock and freeze
  check; editing a block's content can change objects on a locked layer inside it
  ([ACTIVE_DRAWING_SPACE.md](ACTIVE_DRAWING_SPACE.md), section 4). Workaround: lock by not editing blocks
  while a protective drawing space is active.
- Layers are referenced by name in drawing spaces. A layer renamed by hand-editing the DXF shows as
  `missing` in the space.
- A saved panel arrangement from builds before the Layout panel was removed or Properties moved is
  not migrated; it falls back to the default arrangement (other preferences are kept).
