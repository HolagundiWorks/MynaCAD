# Electrical take-off hand-off (MynaOps)

MynaCAD records switchboard and circuit rows in the same project-file take-off as walls and openings
(`takeoff.record`). Kinds **10** (board) and **11** (circuit) map in MynaOps to `electrical` and `electrical_circuits`.

## MynaOps

`OspTakeoff` maps kind 10 to `electrical` and kind 11 to `electrical_circuits`. `CadImporter`
imports them like other take-off rows.

## MynaCAD

`takeoff.record` emits board and circuit JSON rows when the drawing has electrical layout data.
See `crates/engine/src/cmd/takeoffrec.rs` and tests in `takeoff_record.rs`.
