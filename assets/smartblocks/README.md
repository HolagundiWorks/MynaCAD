# Smart block library

MynaCAD's own parametric block definitions (`osas-smartblocks` version 1), one file per discipline:
`rcc.json`, `structural.json`, `plumbing.json`, `hvac.json`, `electrical.json`, `interior.json`.
They are built into the program (`crates/engine/src/cmd/smartblock.rs`) and used by
`smartblock.list`, `smartblock.insert`, `smartblock.get` and `smartblock.edit`. The format and
the command reference are in `docs/smart-blocks.md`.

All definitions are original work by MynaCAD contributors (MIT OR Apache-2.0). Geometry is in
millimetres. They are drafting aids: sizes, cover and reinforcement come from the design.
