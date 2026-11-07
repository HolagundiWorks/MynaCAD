# Plumbing take-off hand-off (Falcon)

Tren records plumbing rows in the same project-file take-off as walls, openings and the electrical
tables (`takeoff.record`, also after a drawing save that has plumbing data). Two new record kinds:

| Kind | Falcon section | One row per | Columns |
|---|---|---|---|
| **12** | `plumbing` | source and service (a tank on supply, a chamber on drain, a riser on either) | `ref` (`PS<handle>-SUP` or `-DRN`), `handle`, `source` (`TK-01`), `kind` (`TK`, `IC`, `RS`), `service` (`SUP`, `DRN`), `european_wc`, `indian_wc`, `basin`, `sink`, `shower`, `floor_trap` (counts the source serves), `pipe_m` (metres of pipe it feeds), `level` |
| **13** | `plumbing_fixtures` | fixture | `ref` (`PF<handle>`), `handle`, `fixture` (`WC-01`), `kind` (`WC`, `IWC`, `WB`, `KS`, `SH`, `FT`), `supply` and `drain` (the source ids, empty when none), `level` |

The rows come from `plumbing.connect` (or `plumbing.update`), which stores them in the drawing's
`plumbing` project data; run it after changing the pipes. A fixture that reaches two sources of one service
is left out of the counts and flagged in the drawing, as electrical does for a point that reaches two boards.

## Falcon

`OspTakeoff` maps kind 12 to `plumbing` and kind 13 to `plumbing_fixtures`, and `BridgeFile` exposes them as
`Plumbing` and `PlumbingFixtures` (test: `Plumbing_rows_are_read_and_do_not_disturb_the_import_of_the_rest`).
`CadImporter` does **not** import them yet: Falcon has no plumbing sheet to put them in, so a take-off with
plumbing rows reads cleanly and the rows wait in the file. Next step on the Falcon side: choose the sheets
(a fixture schedule and a pipe schedule) and map the columns above in `CadImporter`.

## Tren

See `crates/engine/src/cmd/plumbing.rs`, the kinds in `bridge.rs`, `takeoffrec.rs` and `crates/odp/src/import.rs`
(`ROW_KINDS`: a kind missing there is silently dropped when the take-off is written to the `.osp`), and the tests
in `crates/engine/tests/plumbing.rs`.
