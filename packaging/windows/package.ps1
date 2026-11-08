<#
.SYNOPSIS
  Build, sign and package Tren for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    o-draft-<version>-windows-<arch>.msi          per-machine installer (WiX v5)
    o-draft-<version>-windows-<arch>-portable.zip   o-draft.exe, aad-cli.exe, and on x64 the WinUI shell (ODraft.WinUi.exe)

  The binaries link the C runtime statically (+crt-static), so neither the MSI nor the portable
  zip needs the Visual C++ redistributable. The WinUI shell is self-contained for the Windows
  App SDK. Signing is delegated to sign.ps1 (skipped with a warning when no signing secrets are set).

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (rc.exe, signtool.exe),
  .NET 8 SDK (x64 WinUI shell), and WiX v5 with its UI extension (the setup wizard):
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x86 -SkipBuild
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild,
  # Build with the local language model runtime and ship the model file (needs CMake, a C++ toolchain
  # and LLVM libclang; the model comes from $env:AAD_MODEL_FILE or the AAD models folder in APPDATA).
  [switch] $Assistant
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:AAD_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $env:AAD_BUILD_SHA) { $env:AAD_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:AAD_BUILD_DATE) { $env:AAD_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }

Write-Output "Tren $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  # Fail the build (rather than warn) if the icon/VERSIONINFO can't be embedded.
  $env:AAD_REQUIRE_WINRES = '1'
  $Features = @()
  if ($Assistant) {
    # WinUI loads aad_host.dll; the CLI and optional egui host need their own feature flags.
    $Features = @('--features', 'aad/assistant,aad-cli/assistant,aad-host/assistant')
    $Llvm = 'C:\Program Files\LLVM\bin'
    if (-not $env:LIBCLANG_PATH -and (Test-Path $Llvm)) { $env:LIBCLANG_PATH = $Llvm }
    # llama.cpp must use the same static C runtime as the rest of the program (+crt-static above).
    $env:LLAMA_STATIC_CRT = '1'
    $env:CMAKE_MSVC_RUNTIME_LIBRARY = 'MultiThreaded'
  }
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p aad -p aad-cli -p aad-host --target $Target @Features }
  if ($Arch -eq 'x64') {
    $WinUiPublish = Join-Path $TargetDir "aad-winui-publish\$Arch"
    Remove-Item -Recurse -Force $WinUiPublish -ErrorAction SilentlyContinue
    Invoke-Native 'dotnet publish Aad.WinUi' {
      dotnet publish (Join-Path $Root 'apps\aad-winui\Aad.WinUi.csproj') -c Release -p:Platform=x64 -r win-x64 --self-contained true -o $WinUiPublish
    }
  }
}

$Bin = Join-Path $TargetDir "$Target\release"

# Check both binaries' PE headers before packaging. Machine (COFF header) must match -Arch, so an
# x64 build can never ship labelled arm64. Subsystem (optional header): 2 = Windows GUI, 3 = console.
# The app must be GUI (no console window opens with it); the CLI must stay console so its output
# reaches the terminal.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
# These are the cargo build outputs: the app binary is still aad.exe here and is staged as o-draft.exe below.
foreach ($check in @(@('aad.exe', 2), @('aad-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
# The cargo binary is still called aad; it ships as o-draft.exe.
Copy-Item (Join-Path $Bin 'aad.exe') (Join-Path $Stage 'o-draft.exe')
Copy-Item (Join-Path $Bin 'aad-cli.exe') $Stage
# Licence texts and notices, installed beside the program (the licence page of the wizard shows LICENSE-MIT).
foreach ($n in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD_PARTY_LICENSES.md', 'ATTRIBUTION.md') { Copy-Item (Join-Path $Root $n) $Stage }
# Standard furniture DXF catalog (CC0 plan blocks) beside the program.
$FurnSrc = Join-Path $Root 'assets\furniture\standard'
if (Test-Path $FurnSrc) {
  $FurnStage = Join-Path $Stage 'furniture\standard'
  New-Item -ItemType Directory -Force -Path $FurnStage | Out-Null
  Copy-Item (Join-Path $FurnSrc '*') $FurnStage -Recurse -Force
}
# Discipline block libraries (CC0 plumbing, HVAC, electrical, structural and interior DXFs).
$BlocksSrc = Join-Path $Root 'assets\blocks'
if (Test-Path $BlocksSrc) {
  $BlocksStage = Join-Path $Stage 'blocks'
  New-Item -ItemType Directory -Force -Path $BlocksStage | Out-Null
  Copy-Item (Join-Path $BlocksSrc '*') $BlocksStage -Recurse -Force
}

$WinUiDir = $null
$WinUiDefine = @()
if ($Arch -eq 'x64') {
  $WinUiPublish = Join-Path $TargetDir "aad-winui-publish\$Arch"
  if (-not (Test-Path (Join-Path $WinUiPublish 'ODraft.WinUi.exe'))) {
    throw "WinUI publish output missing at $WinUiPublish (build without -SkipBuild, or publish Aad.WinUi first)"
  }
  $HostDll = Join-Path $Bin 'aad_host.dll'
  if (-not (Test-Path $HostDll)) { throw "aad_host.dll missing at $HostDll" }
  $WinUiDir = Join-Path $Stage 'winui'
  Remove-Item -Recurse -Force $WinUiDir -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force -Path $WinUiDir | Out-Null
  Copy-Item (Join-Path $WinUiPublish '*') $WinUiDir -Recurse -Force
  Copy-Item $HostDll $WinUiDir -Force
  # o-draft.exe looks for ODraft.WinUi.exe beside itself (and aad_host.dll next to that exe).
  Copy-Item (Join-Path $WinUiDir 'ODraft.WinUi.exe') $Stage -Force
  Copy-Item $HostDll $Stage -Force
  $WinUiDefine = @('-d', "WinUiDir=$WinUiDir")
  Write-Output "winui: staged from $WinUiPublish"
}

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'o-draft.exe') (Join-Path $Stage 'aad-cli.exe')
if (Test-Path (Join-Path $Stage 'ODraft.WinUi.exe')) {
  & (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'ODraft.WinUi.exe') (Join-Path $Stage 'aad_host.dll')
}
# WiX and the portable zip also take binaries from WinUiDir; sign those copies too.
if ($WinUiDir -and (Test-Path (Join-Path $WinUiDir 'ODraft.WinUi.exe'))) {
  & (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $WinUiDir 'ODraft.WinUi.exe') (Join-Path $WinUiDir 'aad_host.dll')
}

# Typography presets: AAD_FONTS_DIR holds the font files (.ttf/.otf) and their licence texts, taken
# from the upstream releases (nothing is committed to this repository). They are installed in
# <install folder>\fonts, which the application searches before the system fonts, so the build
# works offline. Without the variable the installer carries no fonts and the interface uses the
# system font until the files are added.
$FontsDefine = @()
if ($env:AAD_FONTS_DIR) {
  $Fonts = Join-Path $Stage 'fonts'
  New-Item -ItemType Directory -Force -Path $Fonts | Out-Null
  $files = Get-ChildItem -Path $env:AAD_FONTS_DIR -Recurse -File | Where-Object { $_.Extension -in '.ttf', '.otf' -or $_.Name -match '^(OFL|LICEN[CS]E|COPYRIGHT|NOTICE|SOURCES)' }
  if (-not ($files | Where-Object { $_.Extension -in '.ttf', '.otf' })) { throw "AAD_FONTS_DIR ($env:AAD_FONTS_DIR) has no .ttf or .otf files" }
  if (-not ($files | Where-Object { $_.Name -match '^(OFL|LICEN[CS]E)' })) { throw "AAD_FONTS_DIR ($env:AAD_FONTS_DIR) has no licence file: every bundled font needs its licence" }
  foreach ($f in $files) { Copy-Item $f.FullName (Join-Path $Fonts $f.Name) -Force }
  $FontsDefine = @('-d', "FontsDir=$Fonts")
  Write-Output "fonts: $(@($files).Count) file(s) from $env:AAD_FONTS_DIR"
}

# The assistant's language model: copied beside the program (<install folder>\models), where the
# application looks for it first after AAD_MODEL and the settings folder. Only with -Assistant.
$ModelDefine = @()
if ($Assistant) {
  $ModelFile = if ($env:AAD_MODEL_FILE) { $env:AAD_MODEL_FILE } else { Join-Path $env:APPDATA 'AAD\models\Qwen3-1.7B-Q4_K_M.gguf' }
  if (-not (Test-Path $ModelFile)) { throw "the model file $ModelFile is missing: download it or set AAD_MODEL_FILE" }
  if ((Get-Item $ModelFile).Length -lt 100MB) { throw "$ModelFile is too small to be the model" }
  $Models = Join-Path $Stage 'models'
  New-Item -ItemType Directory -Force -Path $Models | Out-Null
  Copy-Item $ModelFile (Join-Path $Models 'Qwen3-1.7B-Q4_K_M.gguf') -Force
  $ModelDefine = @('-d', "ModelDir=$Models")
  Write-Output "assistant: model $([math]::Round((Get-Item $ModelFile).Length / 1MB)) MB from $ModelFile"
}

# ---- MSI ---------------------------------------------------------------------------------------
# Two editions: o-draft-<version> (the drafting application) and o-draft-ai-<version> (also the local
# assistant and its model). They are alternatives of one product: installing either replaces the
# other in the same folder.
$Edition = if ($Assistant) { "o-draft-ai" } else { "o-draft" }
$ProductName = if ($Assistant) { "Tren with AI assistant" } else { "Tren" }
$Msi = Join-Path $Dist "$Edition-$Version-windows-$Arch.msi"
$LicenseRtf = Join-Path $PSScriptRoot 'EULA.rtf'
$DialogBmp = Join-Path $PSScriptRoot 'WixUIDialogBmp.bmp'
$BannerBmp = Join-Path $PSScriptRoot 'WixUIBannerBmp.bmp'
foreach ($need in @($LicenseRtf, $DialogBmp, $BannerBmp)) {
  if (-not (Test-Path $need)) { throw "installer asset missing: $need" }
}
# Also ship the EULA text beside the program (wizard shows the RTF; install folder keeps a copy).
Copy-Item $LicenseRtf (Join-Path $Stage 'EULA.rtf') -Force
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'aad.wxs') -arch $Arch -ext WixToolset.UI.wixext `
    -d "Version=$MsiVersion" -d "ProductName=$ProductName" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\aad.ico')" `
    -d "LicenseRtf=$LicenseRtf" -d "DialogBmp=$DialogBmp" -d "BannerBmp=$BannerBmp" `
    @FontsDefine @ModelDefine @WinUiDefine -o $Msi
}
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\$Edition-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
if (Test-Path (Join-Path $Stage 'aad_host.dll')) { Copy-Item (Join-Path $Stage 'aad_host.dll') $Portable }
if ($WinUiDir -and (Test-Path $WinUiDir)) {
  Get-ChildItem -Path $WinUiDir -File | ForEach-Object {
    Copy-Item $_.FullName $Portable -Force
  }
  Get-ChildItem -Path $WinUiDir -Directory | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $Portable $_.Name) -Recurse -Force
  }
  # Prefer the signed Stage copies over whatever WinUiDir last wrote for these names.
  foreach ($n in 'ODraft.WinUi.exe', 'aad_host.dll', 'o-draft.exe', 'aad-cli.exe') {
    $src = Join-Path $Stage $n
    if (Test-Path $src) { Copy-Item $src $Portable -Force }
  }
}
if (Test-Path (Join-Path $Stage 'fonts')) { Copy-Item (Join-Path $Stage 'fonts') $Portable -Recurse }
if (Test-Path (Join-Path $Stage 'models')) { Copy-Item (Join-Path $Stage 'models') $Portable -Recurse }
foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD_PARTY_LICENSES.md', 'ATTRIBUTION.md') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
if (Test-Path (Join-Path $Stage 'EULA.rtf')) { Copy-Item (Join-Path $Stage 'EULA.rtf') $Portable }
# Builds made with craft-fonts (CRAFT_FONTS_DIR) embed its fonts: ship their licences.
if ($env:CRAFT_FONTS_DIR) {
  foreach ($ofl in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -ErrorAction SilentlyContinue) {
    Copy-Item $ofl.FullName (Join-Path $Portable "OFL-$($ofl.Directory.Name).txt")
  }
}
$Zip = Join-Path $Dist "$Edition-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# Smoke-test the CLI when this machine can run it. An ARM64 build made on an x64 runner can't run
# here; .github/workflows/windows-arm64.yml installs and runs it on ARM64 instead.
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -ne 'arm64' -or $HostArch -eq 'arm64') {
  Invoke-Native 'aad-cli --version' { & (Join-Path $Stage 'aad-cli.exe') --version }
} else {
  Write-Output "skipping aad-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Msi, $Zip | Format-Table Name, Length
