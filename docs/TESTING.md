# Testing strategy

How MynaCAD is tested, what each layer is for, how to run it, and how to add to it. Windows is the
supported platform for now.

## Run everything locally

```text
cargo xtask ci                                   # the gate: fmt, clippy -D warnings, all tests,
                                                 # asset attribution, layering, licences, UI rules, wasm compile
powershell -File tools/quality/baseline.ps1      # the same commands with timings, as a report
cargo bench -p aad-geom --bench geometry    # benchmarks (see PERFORMANCE.md)
./scripts/takeoff-e2e-harness.sh            # take-off → project file quantity-trust (Linux/cloud; see PROJECT-FILE.md)
```

`cargo xtask ci` is what the pull-request workflow (`.github/workflows/cad-ci.yml`) runs, so a green
local run is a green CI run, apart from machine differences.

## The layers

| Layer | Where | What it checks | Run |
|---|---|---|---|
| Unit | `src/**` (`#[cfg(test)]`) | One function or module: geometry, parsing, units, snapping, command validation | `cargo test -p <crate>` |
| Crate integration | `crates/*/tests/` | A feature through its public API: commands, forms, sheets, xrefs, schedules, take-off | `cargo test -p <crate> --test <name>` |
| Workflow | `apps/aad-cli/tests/` | Real files and real processes: project folders, floors, Xrefs, batch plot, DWG isolation | `cargo test -p aad-cli` |
| Auditor v0 (detect-only) | `crates/auditor`, agent/MCP wrappers, `aad-cli auditor` | Drawing `audit`/`validate` with `fix: false`, command/menu parity, WinUI token scan; combined JSON report; no auto repair | `cargo test -p aad-auditor`; `cargo test -p aad-agent`; `cargo test -p aad-mcp`; `aad-cli auditor --sample --checks audit,validate` |
| UI | `crates/ui-egui/tests/` | Dialogs, panels, widgets, docking, the click-driven drawing workflow and the menu walk through `egui_kittest` (see [UI_TESTING_STRATEGY.md](UI_TESTING_STRATEGY.md)) | `cargo test -p aad-ui-egui` |
| Property-based | `crates/geom/tests/properties.rs`, `crates/engine/tests/properties.rs` | Invariants over generated inputs (below) | `PROPTEST_CASES=20000 cargo test -p aad-engine --test properties` |
| Mutation (file readers) | `crates/io/tests/mutation_robustness.rs` | Corrupted copies of valid DXF files never panic, hang or break writing back | `AAD_MUTATION_ITERS=4000 cargo test -p aad-io --test mutation_robustness` |
| Isolation (DWG) | `apps/aad-cli/tests/dwg_isolation.rs` | Corrupted DWG files cannot take the program down | `cargo test -p aad-cli --test dwg_isolation` |
| Benchmarks | `crates/*/benches/` and `aad-cli perf` | Time of selection, snapping, editing, display-list build, DXF read and write | `cargo bench -p ...` |

### Property-based tests

They state what must be true for *every* input in a range and let `proptest` look for a counter
example, then shrink it. Numbers are compared with tolerances that scale with the inputs.

- **Geometry:** translation and rotation keep lengths, areas and distances; a transform composed
  with its inverse is the identity; mirroring twice is the identity; line-line, line-circle and
  circle-circle intersections lie on both curves and do not depend on argument order; polygon area
  ignores position and rotation and changes sign when reversed; scaling a polyline scales length
  and area as expected; bounds contain their points; a bulge arc joins its two points; no function
  panics for any `f64`, including NaN and infinity.
- **Engine:** no command, given any generated JSON, returns the internal-error result that marks a
  caught panic (`Session::execute` catches panics, so this is the only way a hidden one shows);
  a command that fails leaves the drawing exactly as it was; undoing every edit restores the
  original drawing and redoing them restores the edited one; moving by `d` and back by `-d`
  restores the geometry.

A failure is saved in `<test>.proptest-regressions` next to the test. Commit that file: it is a
permanent regression case that is replayed on every run.

### Mutation tests

A valid file (`examples/apartment.dxf` and a small hand-made structure) is corrupted: truncated,
bytes flipped, lines deleted, duplicated or swapped, numbers replaced by NaN, infinity, huge or
non-numeric text, sections cut, group codes inserted. Each corrupted file goes through reading,
rendering, extents and writing back, on its own thread with a 20 s limit. The run is deterministic
(`AAD_MUTATION_SEED`, `AAD_MUTATION_ITERS`). A failing input is saved to
`target/quality/crashes/` and named in the failure message. Because an allocation failure aborts
the whole process, the file being read is written to `last-mutant.*` first, so after an abort that
file is the culprit.

This is mutation testing around known-good files, not coverage-guided fuzzing. `cargo-fuzz` needs a
nightly toolchain and is not installed here; adding targets for the DXF reader, the entity
deserialiser and the hatch and polyline code is open work (see the backlog), and the mutation
corpus above is the seed corpus for them.

### DWG

DWG is parsed by a third-party library that aborts the process on some damaged files, which no
in-process test can survive. DWG is therefore read in a worker process
(`aad_dwg::isolated`) and tested through it; see BUG-001.

## Adding a regression test for a bug

1. Make the smallest input that shows the bug. Put a file in a `fixtures/` folder next to the test
   (`apps/aad-cli/tests/fixtures/` for files) and add a row to its `README.md` saying how it
   was made and what must happen. If it is an asset-like file, add a row to `ATTRIBUTION.md`.
2. Write the test and **run it on the unfixed code**; it must fail. Keep the output.
3. Make the smallest fix that makes it pass. Do not refactor around it.
4. Run the test, the tests of the same crate, then `cargo xtask ci`.
5. Add the entry to [BUG_BACKLOG.md](BUG_BACKLOG.md): description, root cause (confirmed or
   hypothesis), fix commit, the test, the evidence, remaining risk.

## AI-assisted debugging

Claude (or any assistant) works on a failure in this order, and never merges or releases its own
change:

1. **Reproduce.** Run the failing test, copy the exact failure, record the commit and whether it
   also fails on `main`. A failure that cannot be reproduced is reported as unreproduced.
2. **Investigate.** Trace the path from the failing test to the cause; say which statements are
   confirmed (a backtrace, a failing assertion) and which are hypotheses.
3. **Design.** Name the root cause, the smallest change and what it could affect.
4. **Implement** on a branch or worktree: regression test first, minimal patch, no unrelated
   cleanup, no change to file formats or public API without saying so.
5. **Verify.** The regression test, the crate's tests, `cargo xtask ci`, and the benchmarks if the
   change is on a hot path. A failing test is investigated, never disabled or loosened.
6. **Report** with the template in the next section, and mark anything unverified as unverified.

Needs a human decision first: deleting or rewriting user data, changing production configuration
or signing, adding a major dependency, changing the architecture, merging to a protected branch,
releasing.

### Report template

```text
Bug:            <one line>
Root cause:     <confirmed / hypothesis> <where>
Files changed:  <list>
Fix:            <summary>
Tests added:    <names>
Commands run:   <exact commands>
Results:        <actual output summary: counts, exit codes>
Before / after: <benchmark numbers, if relevant>
Remaining risk: <what is still unverified>
Commit / PR:    <hash>
```

## Continuous integration

- `cad-ci.yml` (pull requests, pushes to `main`): `cargo xtask ci`, then a larger mutation run
  (600 DXF mutants, 40 DWG), then a release build of the program and the command-line tool.
- `cad-nightly.yml` (scheduled): long mutation runs with a new seed every night, the DWG worker
  run, and a dependency audit (`cargo audit`); it writes a summary and keeps any failing input as
  an artifact.
- `release.yml` and `packaging-lint.yml` are unchanged.

The workflows were written and their commands run locally, but **have not yet run on GitHub**;
treat them as unverified until the first run is green.

## Known limits of this strategy

- No coverage measurement yet, so "how much is tested" is judged by the layers above, not a number.
- No automated visual comparison; screenshots were used by hand during development.
  Auditor v0 documents the Windows `ui.screenshot` / MCP `screenshot` hook in its JSON report
  but does not run screenshot golden CI on Linux.
- UI tests are headless; GPU rendering is not under test.
- The DWG worker has no memory cap.
- Large-drawing stress (1M entities) is a benchmark (`AAD_BENCH_LARGE=1`, `aad-cli perf`), not
  a pass/fail test.

## Layer protection, drawing spaces and workspaces (tests)

| File | Checks |
|---|---|
| `crates/engine/tests/layer_protection.rs` | 22 editing calls, by handle and by selection, on a locked and on a frozen layer change nothing; the same calls work on an unlocked drawing (so "refused" is not a bad call); drawing on a locked layer is refused |
| `crates/engine/tests/drawing_space.rs` | independent visible/frozen/locked, switching back and forth restores states, geometry untouched, protection after activation, dirty status, rename/delete/new layers, current-layer rules, undo, validation, hostile parameters, floors |
| `crates/param` (unit) and `crates/engine/tests/param.rs` | Graph units/formulas/cycles/cache; wall preview → commit → undo; stale detection; hostile params; DXF round-trip; bridge take-off rows ([parametric.md](parametric.md)) |
| `apps/aad-cli/tests/drawing_space_file.rs` | spaces and layer states survive a DXF save and reopen, and still protect |
| `crates/ui-egui/tests/context_bar.rs` | both workspaces show the bar; layer, floor and space selectors by clicks; editor saves the saved space only; large changes ask first |
| `crates/ui-egui/tests/workspace_modes.rs` | per-mode arrangement, pin and toolbar state; reset; broken saved slot; tab switching leaves selection and command state alone |
| `apps/aad/src/workspace.rs` (unit tests) | each mode's slot survives a restart |
