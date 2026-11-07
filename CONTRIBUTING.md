# Contributing to Tren

Tren is open source under MIT OR Apache-2.0. Contributions are
welcome under the same terms.

1. Read [CLAUDE.md](CLAUDE.md): scope (2D only), the never-crash rules, clean-room and asset rules.
2. Follow [docs/documentation-policy.md](docs/documentation-policy.md): assess documentation impact, update canonical docs with the code, and use the pull-request checklist there.
3. Build and test with `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layering, licences, UI rules, wasm). Disclose any tests you did not run.
4. New behaviour is a command with tests; update [docs/status.md](docs/status.md) when functionality lands (describe only what exists and is verified).
5. Do not add proprietary third-party CAD vendor material (fonts, patterns, icons, templates, help text) or GPL/LGPL/AGPL code.

Report bugs and security issues through GitHub issues; for a security issue, open a private advisory.

## Keeping the repository clean

- Generated output stays out of git: `target/`, `dist/`, `log/`, `temp/`, `plan/` are ignored. Do
  not commit build output, installers, screenshots, benchmark or profiler output, or logs. Put
  screenshots and scratch files outside the working tree (the asset check fails on stray images).
- Release files go in `dist/` (ignored, kept). Keep the latest validated installer until its
  replacement has been built and validated.
- Do not run `cargo clean` as part of normal work. It can remove all of `target/`, not only what
  you name; to free space, delete `target/debug` by hand ([docs/BUILD_AND_DEVELOPMENT.md](docs/BUILD_AND_DEVELOPMENT.md)).
- A new dependency needs a use in the code of the crate that declares it. Remove a dependency in its
  own commit, separate from version upgrades, after `cargo check --workspace --all-targets`.
- Every new file asset needs a row in `ATTRIBUTION.md`; every new document is linked from
  `README.md`; a document describes what exists, not what is planned (planned work goes in
  `docs/status.md`).
- Before deleting a file, find its references (`grep`, `#[path]`, scripts, workflows) and record why
  in the commit message.

## Licence of contributions

Tren is licensed under the MIT License or the Apache License 2.0, at the user's option (see
[LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) and [NOTICE](NOTICE)). Unless you say
otherwise, any contribution you submit for inclusion is licensed under the same two licences,
without additional terms or conditions, as the Apache License 2.0 section 5 describes. Only submit
work you have the right to license this way: your own, or material under a compatible open licence
that you record in [ATTRIBUTION.md](ATTRIBUTION.md) or [NOTICE](NOTICE). Never submit code copied
from GPL, LGPL or AGPL projects, or material taken from closed-source CAD products (see CLAUDE.md).
