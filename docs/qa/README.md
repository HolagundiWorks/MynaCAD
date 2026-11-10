# MynaCAD QA and performance campaign

Started: 2026-10-26. Working tree includes uncommitted UI consistency, polar tooltip,
eTransmit removal, and dynamic door/window work on top of `2f4af48`.

## Rules

- Numbers and pass/fail rows in these files are **measured or observed in this campaign**.
- Rows marked `not run`, `blocked`, or `hypothesis` are not claimed as verified.
- Pre-existing baselines in [PERFORMANCE.md](../PERFORMANCE.md) and [KNOWN_ISSUES.md](../KNOWN_ISSUES.md)
  are cited, not rewritten to look better.
- Related maintained docs stay authoritative for strategy; this folder is the campaign ledger.

## Deliverables

| File | Contents |
|---|---|
| [environment.md](environment.md) | Toolchain, hardware, build configs |
| [screen-inventory.md](screen-inventory.md) | Screens, dialogs, panels |
| [test-matrix.md](test-matrix.md) | Cases, status, evidence links |
| [defect-ledger.md](defect-ledger.md) | Defects with IDs |
| [performance-baseline.md](performance-baseline.md) | This-machine measurements |
| [performance-profile.md](performance-profile.md) | Bottlenecks with traces |
| [optimization-log.md](optimization-log.md) | Changes with before/after |
| [compatibility-report.md](compatibility-report.md) | DXF/DWG coverage |
| [regression-report.md](regression-report.md) | Builds and automated tests |
| [visual-audit.md](visual-audit.md) | Screenshots and UI notes |
| [release-readiness.md](release-readiness.md) | Blockers and open risks |
| [artifacts/](artifacts/) | Raw command output |

## Campaign status (2026-10-26)

| Phase | Status |
|---|---|
| A Baseline | **partial** — env, lib suites, perf 1k/10k/100k, cold start |
| B Screen inventory | **done** (inventory; testing statuses still `not_tested`) |
| C Screen functional | **partial** — launch + Layer combo; Escape/dirty defects logged |
| D CAD commands / geometry | **partial** — wall/door engine smoke + existing unit tests |
| E Undo / documents | **partial** — one undo/redo smoke; WinUI dirty bug open |
| F DWG/DXF compatibility | **partial** — aad-io lib only |
| G Performance profiling | **not started** (baseline table only) |
| H Optimization | **not started** |
| I Stress / soak | **not started** |
| J Visual / a11y | **partial** — 2 screenshots |
| K Automated regression | **partial** — key crates; not full `xtask ci` |
| L Triage and report | **draft** — see release-readiness.md |

**Definition of Done:** not met. See [release-readiness.md](release-readiness.md).
