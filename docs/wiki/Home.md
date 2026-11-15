# MynaCAD

MynaCAD is the 2D drafting app of the Myna suite: an open-source, pure-Rust drafting engine with a Windows-only WinUI 3 shell. It reads and writes DXF and DWG, draws architectural and structural detail, plots, and records the quantity take-off into the project's `.osp` file for MynaOps.

**Scope: 2D only.** 3D, solver-based constraints and mechanical commands are out of scope. A Grasshopper-style parametric graph for walls and take-off exists separately (`aad-param`).

## Pages
- [[Architecture|Architecture]]
- [[Build and test|Build-and-test]]
- [[Conventions|Conventions]]
- [[Levels, sheets and plotting|Levels-sheets-and-plotting]]
- [[The Myna suite|The-Myna-suite]]

## What exists and what does not
Everything MynaCAD does is a command, and the capability table in `docs/status.md` is the single source for what is implemented and what is only planned. A feature is not listed as implemented unless it exists and has a test.

Built since the 1.8.0 installer (in `main`, in installers from 1.9.0 on; see [[Levels, sheets and plotting|Levels-sheets-and-plotting]] for what was driven by hand): sections and elevations as drawing tabs, a BIM-style sheet index for plotting, take-off level ids and record-all-levels in the shared `.osp`, the Map Layers wizard fix.

Decided, not built: the Ops-first workflow (MynaOps starts first, creates or opens the project and launches MynaCAD with it; MynaCAD keeps only import and export; project configuration is done in MynaOps) and a live sync between the two running apps with on/off and Sync now buttons. Design: `docs/ops-first-workflow.md` and `docs/live-sync.md` once merged.

Built so far for the Ops-first workflow: the project configuration schema (`meta.config.*`), its Rust and C# libraries and the `project.config.apply` engine command (tested with shared fixtures; see `docs/ops-first-workflow.md` section 10). The MynaOps screens, managed mode and launch are not built.
