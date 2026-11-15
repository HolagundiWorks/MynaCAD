# Shared configuration fixtures

The same files are committed in MynaCAD (`crates/odp/tests/fixtures/config`) and MynaOps (`BBSDesktop/Office.Shared.Tests/Fixtures/config`).
Rust (`aad-odp::config`, `config_tests.rs`) and C# (`Office.Shared.ProjectConfig`, `ProjectConfigTests.cs`) read every `*.json` here and must agree.
Change a file in both repos or in neither.

Each `*.json` is `{ "description", "expect", "keys", ... }`:

| `expect` | Meaning | Also checked |
|---|---|---|
| `ok` | the keys read as a configuration | `levels`, `tabs`, `rev` (counts and numbers), `notes` (how many keys were left out) |
| `error` | the whole configuration is rejected, no panic | `error_key`: the meta key the error names starts with this |
| `too_new` | `config.schema.min_reader` is newer than the reader | |

`keys` maps a short key name (`levels` is the meta key `config.levels`) to the key's JSON object, or to a string that is used as the raw
text of the key (for text that is not JSON). Unknown names are ignored by a reader.

Oversize and deeply nested values, thousands of levels and byte-level mutations are generated in the tests of both languages, not committed.

`config-rust.osp` was written by the Rust writer from `valid/full.json` (plus a `config.applied.cad` report, a local key), and
`config-csharp.osp` by the C# writer from the same file; each language reads the other's file and rewriting the same configuration over
either changes nothing. Regenerate them with `cargo test -p aad-odp regenerate_rust_fixture -- --ignored` and, in MynaOps,
`CONFIG_FIXTURE_OUT=<folder> dotnet test --filter Regenerate_the_csharp`; then copy both files to both repos.
