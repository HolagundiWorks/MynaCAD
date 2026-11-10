# Test fixtures

Every file here is made for the tests, contains no third-party drawing content, and says how it was
made and what it must do. Fixtures are read-only inputs: tests copy them before changing anything.

| File | Made by | Expected behaviour |
|---|---|---|
| `dwg/alloc-bomb-ac18-section-size.dwg` | `examples/apartment.dxf` written to DWG by MynaCAD, then corrupted by `crates/io/tests/mutation_robustness.rs` (seed in that test's history; 15 019 bytes). Against `acadrust` 0.6.3 a damaged AC18 section size made the reader ask for ~3.4 petabytes and abort (`0xc0000409`). Against current `acadrust` 0.6.8 the same bytes open as an empty drawing (no abort) | Through the isolated worker (`aad_dwg::isolated`) the parent must **not** abort: success or an ordinary error both satisfy BUG-001. See `docs/BUG_BACKLOG.md` |
