# Build and test

Requirements: Rust (stable), .NET 8 SDK and the Windows App SDK for the WinUI shell. The sibling folder `MynaUI` must sit next to this repository.

```
cargo xtask ci          # fmt, clippy -D warnings, tests, assets, layering, licences, UI design system, wasm
cargo xtask layers      # layering only
cargo run --release -p aad -- --sample --control 7777
```

Build the WinUI shell:

```
cargo build -p aad-host
dotnet build apps/aad-winui/Aad.WinUi.csproj -c Debug -p:Platform=x64 -o target/aad-winui
```
Copy `aad_host.dll` beside the executable.

## Driving the app
`--control PORT` opens a loopback JSON-lines channel on `127.0.0.1:PORT`: `cmdline.input` types at the command line, `engine.execute` runs a command from JSON, `ui.screenshot` writes a PNG. Headless: `aad-cli run --sample --script 'LINE 0,0 10,10\n' --export out.png`.

## Parallel work
Use a separate `CARGO_TARGET_DIR` per checkout or agent.
