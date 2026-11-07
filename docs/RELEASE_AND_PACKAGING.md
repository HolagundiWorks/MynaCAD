# Release and packaging

Windows is the only supported platform. The macOS, Linux and web steps exist as scripts and CI jobs
for later phases and were **not exercised in this audit**. The step-by-step release procedure and
the list of release artifacts and secrets is in [releasing.md](releasing.md); this page adds the
parts an audit needs.

## Version

The single source is `[workspace.package] version` in the root `Cargo.toml` (currently 0.6.0).
`cargo xtask version` prints it. Tags and history are never removed.

## Windows release build (verified 2026-10-12)

```text
powershell -NoProfile -ExecutionPolicy Bypass -File packaging/windows/package.ps1 -Arch x64
powershell -NoProfile -ExecutionPolicy Bypass -File packaging/windows/package.ps1 -Arch x64 -Assistant
```

1. `cargo build --release --locked -p aad -p aad-cli -p aad-host --target x86_64-pc-windows-msvc`
   with a static C runtime, so no Visual C++ redistributable is needed. With `-Assistant`, also
   `aad/assistant`, `aad-cli/assistant` and `aad-host/assistant` (llama.cpp).
2. Stage the files under `target/windows-package/` (licences, EULA, WinUI shell; with `-Assistant`,
   the model under `models\`).
3. Build the MSI with WiX v5 (`packaging/windows/aad.wxs`: Welcome splash bitmaps, open-source EULA,
   Shortcuts) and the portable zip.
4. Sign with `packaging/windows/sign.ps1` if signing material exists; otherwise the artifacts are
   unsigned and the script warns. See **Windows Authenticode** below.
5. Optional SBOM (any host): `cargo xtask sbom --out dist/release/o-draft-<version>.cdx.json`
   (CycloneDX JSON from `cargo metadata`; no secrets). The release workflow attaches this artifact.

## Suite install (O-Suite)

For practice machines that run the whole Kivo family, ship the **portable zip** output of
`package.ps1` into Kivo’s `installer/suite/build-single.ps1` (not the per-machine MSI).
End users normally run **`O-Suite-Setup-<version>.exe`** (Kivo repo): install or remove
Kivo, Falcon, and Tren from one wizard. See Kivo `installer/suite/README.md` and
[docs/SUITE.md](SUITE.md).

Output: `dist/release/o-draft-<version>-windows-x64.msi` and `...-portable.zip`, or with `-Assistant`
`o-draft-ai-<version>-windows-x64.msi` (and matching portable zip).

## Windows Authenticode

`packaging/windows/sign.ps1` tries, in order:

| Mode | How to enable | Notes |
|---|---|---|
| Org PFX | `WINDOWS_CERTIFICATE` (base64 `.pfx`) and optional `WINDOWS_CERTIFICATE_PASSWORD` | Production / CI vault |
| Azure Trusted Signing | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE`, plus `AZURE_SIGNING_ENDPOINT` **or** `AZURE_SIGNING_REGION`; optional `SIGN_BACKEND=ossign\|signtool`, `OSSIGN` | Prefer OSSign when `ossign.exe` is present; copy `ossign.config.example.yaml` locally — never commit filled secrets. Same secret names as OSAS / Kivo / Falcon packaging workflows |
| Dev self-sign | `SIGN_SELF_SIGNED=1` | Creates or reuses `CN=Tren Development (self-signed)` under `%LOCALAPPDATA%\O-Draft\dev-codesign\`; SmartScreen still warns |

With none of the above, the script warns and leaves files unsigned (exit 0). **Never** commit `.pfx`,
`.p12`, client secrets, or a filled `ossign.config.yaml`. Threat model: [threat-model.md](threat-model.md).

```powershell
$env:SIGN_SELF_SIGNED = '1'
pwsh packaging/windows/sign.ps1 dist\release\o-draft.exe dist\release\o-draft.msi
Get-AuthenticodeSignature dist\release\o-draft.exe
```

## Required at run time

The program needs nothing beside the executable ([PROJECT_STRUCTURE.md](PROJECT_STRUCTURE.md)):
icons, patterns, linetypes, the drafting font and the interface strings are compiled in. The AI
edition installs the language model under `<install folder>\models`; a drafting-only install has no
model file.

## Validation

Before a release: `cargo xtask ci`, then the checks in [TESTING.md](TESTING.md), then the installer
checks below.

Performed on 2026-10-12 for the rebuilt 0.6.0 (unsigned, built with `DIST=dist\candidate`, then
moved to `dist/release`):

| Check | Result |
|---|---|
| Build: `package.ps1 -Arch x64` | passed (release build 3.5 minutes); MSI 17 846 272 bytes, zip 22 689 850 bytes |
| Portable zip, extracted to an empty folder | contains `o-draft.exe`, `aad-cli.exe`, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md`; the CLI reports 0.6.0, reads a DXF, draws, saves and exports a PNG; the GUI starts on the GPU canvas, answers the control channel and quits; a damaged DWG (the BUG-001 fixture) is refused with a message by both the CLI and the GUI and the program stays up (the DWG worker works from the packaged executable) |
| MSI administrative extract | the same two executables as the previous build |
| MSI real install (silent, elevated) | exit 0; `C:\Program Files\O-Draft\` with both executables; "Add or remove programs" entry "Tren 0.6.0" by Human Centric Works; Start Menu shortcut; no desktop shortcut (opt-in) |
| Installed programs | CLI runs and saves a DXF; GUI starts, shows 46 entities on the GPU canvas, exits cleanly |
| MSI uninstall (silent) | exit 0; install folder, Start Menu entry and uninstall entry all removed |

Limits of that test: it ran on the development machine, not a clean one, and a Tren installation
already existed (the install upgraded it). It was a silent install, so the wizard pages (Welcome,
Shortcuts) were **not** exercised, nor the desktop-shortcut option, upgrade-over-older-version
behaviour with a real older release, file-type registration (the `.dxf` and `.dwg` "Open with"
entries exist in `aad.wxs` but were not read back from the registry), or the x86 and arm64
variants. Signing was not tested (no secrets).

Findings from the test (2026-10-12):
- That candidate did not surface `NOTICE` / `ATTRIBUTION.md` in the exercised install path. The
  packaging scripts **now** stage `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`,
  `THIRD_PARTY_LICENSES.md` and `ATTRIBUTION.md` into the stage directory, WiX MSI component, and
  portable zip (`package.ps1`, `aad.wxs`). Confirm on the next Windows release candidate.
- Both packages are unsigned until signing secrets are configured; Windows SmartScreen will warn.

## Artifact retention

- `dist/release/` holds the validated 0.6.0 installer and portable zip (SHA-256 of the MSI `11D64F87...BEEAE9`, of the zip `ACFD8145...0772D5`); `dist/release/previous-0.6.0-first-build/` holds the first 0.6.0 build (older code, never installed). `dist/` is ignored by git. Keep
  the latest validated build; do not delete it until a replacement has been built and validated.
- `target/` is generated and may be deleted at any time ([BUILD_AND_DEVELOPMENT.md](BUILD_AND_DEVELOPMENT.md)).
- CI artifacts (crash inputs from the mutation tests) are kept by GitHub for its default period.
