# Defect ledger

IDs: `QA-YYYY-NNN`. Severity: P0 critical, P1 high, P2 medium, P3 low.
Status: open | fixed | wontfix | duplicate | blocked.

Pre-existing issues from [KNOWN_ISSUES.md](../KNOWN_ISSUES.md) are referenced, not duplicated, unless newly evidenced.

| ID | Sev | Summary | Status | Evidence |
|---|---|---|---|---|
| QA-2026-001 | P3 | `vdb.convert` test required `pushed == false` | fixed | artifacts/aad-engine-lib.txt |
| QA-2026-002 | P3 | Mirrored TEXT footprint assert too tight for system fonts | fixed | artifacts/aad-io-lib.txt |
| QA-2026-003 | P3 | New empty drawing shows dirty `Drawing1*` with 0 entities | fixed | artifacts/qa-2026-003-retest.txt |
| QA-2026-004 | P3 | Layer combo stays open after Escape | fixed | artifacts/qa-2026-004-retest.txt |

### QA-2026-001 — vdb.convert test assumes no push

- Severity: P3 (test environment assumption)
- Reproduce: `cargo test -p aad-engine --lib convert_writes_vdb` when `ensure_shared_vdb` succeeds
- Expected: sidecar written; `pushed` may be true if a local daemon connects
- Actual: `assert_eq!(pushed, false)` failed with `true`
- Root cause: `run_convert` calls `ensure_shared_vdb` then `push_if_connected`
- Fix: assert `pushed` is boolean; keep sidecar/count checks (`crates/engine/src/cmd/vdb.rs`)
- Retest: passed ([artifacts/qa-2026-001-retest.txt](artifacts/qa-2026-001-retest.txt)); full engine lib 315/315

### QA-2026-002 — mirrored TEXT extrusion footprint

- Severity: P3 (font-metric-sensitive unit test)
- Reproduce: `cargo test -p aad-io --lib mirrored_text_extrusion_keeps`
- Expected: right-justified at x=10, drawn to the left
- Actual: `max.x ≈ 9.928` (not within 1e-6 of 10.0)
- Root cause: shaped width of `"AB"` depends on installed fonts
- Fix: require `max.x ∈ (9.5, 10.001]` and `min.x < 9.5` (`crates/io/src/tests.rs`)
- Retest: passed; full aad-io lib **56/56**

### QA-2026-003 — dirty flag on empty new drawing

- Severity: P3
- Reproduce: Launch WinUI Debug; observe tab `Drawing1*` while status shows `0 entities`
- Expected: new untitled drawing clean until first edit
- Actual: asterisk present without drawn entities
- Affected: dirty indicator (`MainWindow` uses host `dirty` / `DocState::is_dirty`)
- Root cause: chrome refresh re-applied current layer/floor/units via commands that always
  `doc_mut()` / `Arc::make_mut`, so `!ptr_eq(doc, saved)` even with 0 entities. Confirmed engine
  `Session::new` starts clean (raw output `artifacts/engine-smoke-wall-undo-dxf.txt`, deleted in the 2026-10-12 cleanup).
- Fix:
  - `layer.current`: no-op (no COW) when name already matches `CLAYER`
  - `floor.current`: skip `project::set` when already active
  - `drawing.units` metadata: no-op when unit unchanged
  - `FillCombo` / `SyncDraftingUnits`: avoid redundant `SelectedIndex` / `SelectedItem` writes
  - regression: `making_the_current_layer_again_does_not_dirty_the_drawing`
- Retest: unit test passed; WinUI visual retest in [artifacts/qa-2026-003-retest.txt](artifacts/qa-2026-003-retest.txt)

### QA-2026-004 — Escape does not dismiss Layer combo

- Severity: P3
- Reproduce: Open Layer dropdown on architecture bar; press Escape once or twice
- Expected: combo closes
- Actual: dropdown remained open (observed twice)
- Root cause: `PollEscapeCancel` returned early when focus was a `ComboBox`, and when the flyout
  focused a list item (not the ComboBox) Escape fell through to CAD cancel without closing the
  dropdown.
- Fix: `CloseOpenComboBoxes()` closes Layer/Space/Floor/Units drop-downs from both
  `OnShellKey` Escape and `PollEscapeCancel` before command cancel.
- Retest: [artifacts/qa-2026-004-retest.txt](artifacts/qa-2026-004-retest.txt)
