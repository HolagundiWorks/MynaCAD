# Build and development

Windows 11 x64 is the only platform that is built, tested and packaged. Commands marked
**verified** were run on 2026-10-12 on the machine described in [PERFORMANCE.md](PERFORMANCE.md);
commands marked **untested** are taken from the scripts and not run in this audit.

## Prerequisites

| Need | Version | Notes |
|---|---|---|
| Rust (stable, MSVC target `x86_64-pc-windows-msvc`) | 1.95 or later for the desktop and UI crates; the other crates declare 1.90 | The development machine uses rustc/cargo 1.99.0. Edition 2024 |
| Visual Studio Build Tools (MSVC linker) | any recent | Installed with the Rust MSVC toolchain |
| `wasm32-unknown-unknown` target | for `cargo xtask wasm` | `rustup target add wasm32-unknown-unknown` |
| WiX v5 and `WixToolset.UI.wixext` 5.0.2, Windows SDK (`rc.exe`, `signtool.exe`) | installer only | see [RELEASE_AND_PACKAGING.md](RELEASE_AND_PACKAGING.md) |
| CMake, a C++ toolchain, LLVM `libclang.dll` (`winget install LLVM.LLVM`, then set `LIBCLANG_PATH` to its `bin` folder) | only for the optional `llama` feature of `aad-llm` | **verified**: `cargo build -p aad-llm --features llama` builds in about 2.5 minutes |

No fonts, models or other downloads are needed to build or test. A GPU adapter is needed to run
the application; the headless UI tests render without a window.

## Build

```text
cargo build --workspace                 # debug (covered by `cargo xtask ci`; not run on its own)
cargo build --workspace --release       # verified (3.7 minutes from a clean `target/`)
cargo build --release -p aad       # just the desktop app, verified
# Local assistant (llama.cpp). Needs CMake, MSVC, and LIBCLANG_PATH (see Prerequisites).
LIBCLANG_PATH="C:/Program Files/LLVM/bin" cargo build --release -p aad-host --features assistant
LIBCLANG_PATH="C:/Program Files/LLVM/bin" cargo build --release -p aad-cli --features assistant
LIBCLANG_PATH="C:/Program Files/LLVM/bin" cargo build --release -p aad --features egui,assistant
```

Outputs are in `target/debug` and `target/release` (`aad.exe`, `aad-cli.exe`, `aad_host.dll`; the Windows package ships `aad.exe` as `o-draft.exe`).

### Local model file

The weights are not in the repository. Place `Qwen3-1.7B-Q4_K_M.gguf` (about 1.1 GB, from
`unsloth/Qwen3-1.7B-GGUF`) where the runtime looks ([ai-agent.md](ai-agent.md)):

1. `AAD_MODEL` (full path to the `.gguf`), or
2. `%APPDATA%\MynaCAD\models\Qwen3-1.7B-Q4_K_M.gguf`, or
3. `models\Qwen3-1.7B-Q4_K_M.gguf` beside the program or in the working directory.

Without the `assistant` feature, or without a model file, `/` requests and natural-language
sentences in the command bar report that the assistant is not included or that the model is missing;
ordinary CAD still works.

## Run

```text
cargo run --release -p aad -- --sample            # WinUI shell when target/aad-winui/ODraft.WinUi.exe exists
cargo run --release -p aad --features egui -- --egui --sample   # optional egui window (Windows)
cargo run --release -p aad -- file.dxf            # WinUI opens the file (error if WinUI is missing)
cargo run --release -p aad -- --control 47123     # WinUI + JSON control channel
cargo run --release -p aad-cli -- info examples/apartment.dxf
cargo run --release -p aad-cli --features assistant -- ask --sample "What is in this drawing?"
```

The WinUI shell is built separately. For the command-bar assistant, build the host **with** the
model runtime, then the C# shell (it copies `aad_host.dll` beside the exe):

```text
LIBCLANG_PATH="C:/Program Files/LLVM/bin" cargo build --release -p aad-host --features assistant
dotnet build apps/aad-winui/Aad.WinUi.csproj -c Debug -p:Platform=x64 -o target/aad-winui
```

Without `--features assistant`, `cargo build -p aad-host` still produces a host that runs CAD; `/`
requests then say the assistant is not in the build. That build needs the .NET 8 SDK.

The control channel is described in [control-protocol.md](control-protocol.md).

## Test, format, lint

```text
cargo xtask ci                                           # fmt, clippy -D warnings (with aad/egui), tests, assets, layers, licences, ui, wasm
cargo xtask sbom                                         # CycloneDX SBOM to dist/release/o-draft-<version>.cdx.json (no secrets)
cargo fmt --all -- --check                               # verified
cargo clippy --workspace --all-targets --features aad/egui -- -D warnings
cargo test --workspace --features aad/egui               # keeps the optional egui shell in the graph
cargo test -p aad-ui-egui                                # verified: the UI tests
```

`cargo xtask --help` lists the other tasks. Details, long-running property and mutation tests and
benchmarks: [TESTING.md](TESTING.md).

## Packaging

```text
powershell -NoProfile -ExecutionPolicy Bypass -File packaging\windows\package.ps1 -Arch x64
```

**Verified** on 2026-10-12 with Windows PowerShell 5.1 (PowerShell 7, `pwsh`, is not installed on
the development machine; the script's own examples use it and were not run). It needs WiX 5.0.2
with `WixToolset.UI.wixext` and the Windows SDK. It builds with
`cargo build --release --locked --target x86_64-pc-windows-msvc` and a static C runtime, then writes
`*.msi` and `*-portable.zip` to `dist/release` (set `DIST` to write elsewhere, for example
`$env:DIST = 'dist\candidate'`, to avoid overwriting an existing build). The result was installed,
run and uninstalled: [RELEASE_AND_PACKAGING.md](RELEASE_AND_PACKAGING.md).

## Development workflow

1. Read [CLAUDE.md](../CLAUDE.md) (2D scope, never-crash rules, clean-room and asset rules).
2. Make the change with its test. New behaviour is a command (`crates/engine/src/cmd/`).
3. Run the relevant crate's tests while working, then `cargo xtask ci` before committing.
4. Update [status.md](status.md) when functionality lands, and the relevant document in this folder.
5. A crash or data-loss fix needs a regression test; a performance fix needs a before and after
   number from the same benchmark ([PERFORMANCE.md](PERFORMANCE.md)).

## Disk use

A full debug build with tests is large: a clean rebuild of everything came to 25 GB, and over time the `target/` folder reached about 101 GB on the
development machine (98 GB in `target/debug`). It is generated and safe to delete: everything in
it is rebuilt by `cargo build`, `cargo test` or `cargo xtask ci` (the full `cargo xtask ci` from nothing took 13.5 minutes here, the
release build of the workspace 3.7 minutes; one run each).

**Warning:** in the audit run, `cargo clean --profile dev` followed by
`cargo clean --target wasm32-unknown-unknown` left no `target/` folder at all, including the
release build, the installer staging folder and the benchmark output. Which of the two removed
those was not isolated, so assume any `cargo clean` can remove all of `target/`. To keep the
release build, delete only `target/debug` by hand. Never keep the only copy of anything in `target/`;
release files belong in `dist/` (ignored by git, kept).

Do not clean as part of the normal loop.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `cargo xtask assets` fails with a stray image | An image file in the repository root or an untracked asset. Move screenshots outside the repo; every asset file needs a row in `ATTRIBUTION.md` |
| `cargo xtask layers` fails | A crate depends on a crate from a higher layer; see `xtask/src/layers.rs` |
| `llama-cpp-sys-2` fails with "Unable to find libclang" | You enabled the `llama` feature without LLVM; install LLVM and set `LIBCLANG_PATH`, or build without the feature (the default) |
| A UI test cannot start a gesture at the canvas edge | The press lands on the docked panel's resize area; use a point inside the canvas ([UI_TESTING_STRATEGY.md](UI_TESTING_STRATEGY.md)) |
| `cargo bench` runs for many minutes | The benches build large drawings; run one at a time with `-- --warm-up-time 1 --measurement-time 3` |
