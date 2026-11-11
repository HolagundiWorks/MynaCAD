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

Planned or in progress, not yet shipped: sections and elevations as drawing tabs, a BIM-style sheet index for plotting, take-off level ids in the shared `.osp`.
