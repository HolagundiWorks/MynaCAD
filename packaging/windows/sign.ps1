<#
.SYNOPSIS
  Authenticode-sign files with a PFX, Azure Trusted Signing (OSSign or signtool), or a local self-signed cert.

.DESCRIPTION
  Tries, in order:
    1. WINDOWS_CERTIFICATE (base64 .pfx) + optional WINDOWS_CERTIFICATE_PASSWORD — signtool.
    2. Azure Trusted Signing env vars — OSSign when found, else signtool + Trusted.Signing.Client.
       Optional: AZURE_SIGNING_REGION, SIGN_BACKEND=ossign|signtool, OSSIGN, SIGNTOOL.
    3. SIGN_SELF_SIGNED=1 — create or reuse a local development Code Signing cert under
       %LOCALAPPDATA%\O-Draft\dev-codesign\ and sign with signtool (SmartScreen will still warn).
  With none of the above, warns and leaves files unsigned (exit 0).

.EXAMPLE
  $env:SIGN_SELF_SIGNED = '1'
  pwsh packaging/windows/sign.ps1 dist/o-draft.exe dist/o-draft.msi
#>
param(
  [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
  [string[]] $Files
)
$ErrorActionPreference = 'Stop'

function Write-Warn([string] $Message) {
  if ($env:GITHUB_ACTIONS) { Write-Output "::warning::$Message" } else { Write-Warning $Message }
}

function Find-SignTool {
  if ($env:SIGNTOOL) { return $env:SIGNTOOL }
  $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $found = Get-ChildItem -Path $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $found) { throw 'signtool.exe not found (install the Windows SDK)' }
  return $found.FullName
}

function Find-OsSign {
  if ($env:OSSIGN) {
    if (-not (Test-Path -LiteralPath $env:OSSIGN)) { throw "OSSIGN points to a missing file: $env:OSSIGN" }
    return (Resolve-Path -LiteralPath $env:OSSIGN).Path
  }
  $onPath = Get-Command ossign.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  $local = Join-Path $env:LOCALAPPDATA 'ossign\ossign.exe'
  if (Test-Path -LiteralPath $local) { return (Resolve-Path -LiteralPath $local).Path }
  return $null
}

function Get-AzureSigningRegion {
  if ($env:AZURE_SIGNING_REGION) { return $env:AZURE_SIGNING_REGION.Trim() }
  $ep = $env:AZURE_SIGNING_ENDPOINT
  if ($ep -match 'https?://([a-z0-9-]+)\.') { return $Matches[1] }
  throw 'set AZURE_SIGNING_REGION (e.g. eus) or use a regional AZURE_SIGNING_ENDPOINT host'
}

function Get-OsSignType([string] $Path) {
  switch -Regex ([IO.Path]::GetExtension($Path).ToLowerInvariant()) {
    '^\.msi$' { return 'authenticode' }
    '^\.ps1$' { return 'powershell' }
    default { return 'pecoff' }
  }
}

function Invoke-SignTool([string[]] $Arguments) {
  & $script:SignTool @Arguments
  if ($LASTEXITCODE -ne 0) { throw "signtool $($Arguments[0]) failed with exit code $LASTEXITCODE" }
}

function Assert-HasSignature([string[]] $Paths) {
  foreach ($path in $Paths) {
    $sig = Get-AuthenticodeSignature -FilePath $path
    if (-not $sig.SignerCertificate) {
      throw "no Authenticode signature on $path (status $($sig.Status))"
    }
  }
}

function Ensure-SelfSignedPfx {
  $dir = Join-Path $env:LOCALAPPDATA 'O-Draft\dev-codesign'
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $pfxPath = Join-Path $dir 'o-draft-dev.pfx'
  $pwdFile = Join-Path $dir 'pfx.password'
  $subject = 'CN=O-Draft Development (self-signed)'
  if (-not (Test-Path -LiteralPath $pfxPath) -or -not (Test-Path -LiteralPath $pwdFile)) {
    Write-Output "Creating self-signed code-signing certificate at $pfxPath"
    $cert = New-SelfSignedCertificate `
      -Type CodeSigningCert `
      -Subject $subject `
      -FriendlyName 'MynaCAD development code signing' `
      -CertStoreLocation 'Cert:\CurrentUser\My' `
      -KeyExportPolicy Exportable `
      -KeySpec Signature `
      -KeyLength 2048 `
      -HashAlgorithm SHA256 `
      -NotAfter (Get-Date).AddYears(5)
    $plain = [guid]::NewGuid().ToString('N')
    $secure = ConvertTo-SecureString -String $plain -Force -AsPlainText
    Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $secure | Out-Null
    Set-Content -Path $pwdFile -Value $plain -Encoding ascii
    # Keep a copy in the store for local trust experiments; signing uses the PFX.
  }
  return @{ Path = $pfxPath; Password = (Get-Content -LiteralPath $pwdFile -Raw).Trim() }
}

function Invoke-SignWithPfx([string] $PfxPath, [string] $Password, [string[]] $Paths, [string] $Label) {
  $script:SignTool = Find-SignTool
  $common = @('sign', '/v', '/fd', 'SHA256', '/td', 'SHA256', '/d', 'MynaCAD', '/du', 'https://github.com/HolagundiWorks/O-DRAFT')
  Write-Output "Signing with $Label`: $($Paths -join ', ')"
  $ts = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
  $withTs = $common + @('/tr', $ts, '/f', $PfxPath)
  if ($Password) { $withTs += @('/p', $Password) }
  try {
    Invoke-SignTool ($withTs + $Paths)
  }
  catch {
    Write-Warn "Timestamped sign failed ($($_.Exception.Message)); retrying without a timestamp server"
    $noTs = $common + @('/f', $PfxPath)
    if ($Password) { $noTs += @('/p', $Password) }
    Invoke-SignTool ($noTs + $Paths)
  }
  Assert-HasSignature $Paths
  # /pa requires a trusted root; self-signed usually fails that check — only try for non-dev certs.
  if ($Label -notmatch 'self-signed') {
    Invoke-SignTool (@('verify', '/pa', '/v') + $Paths)
  }
  else {
    Write-Warn 'Self-signed: SmartScreen and "trusted publisher" checks will still warn until you use a CA or Azure Trusted Signing cert'
  }
}

function Invoke-OsSignAzure([string[]] $Paths) {
  $ossign = Find-OsSign
  if (-not $ossign) { throw 'OSSign not found (set OSSIGN or install ossign.exe)' }
  $region = Get-AzureSigningRegion
  $ts = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.acs.microsoft.com' }
  $cfg = Join-Path $script:Tmp 'ossign-config.yaml'
  $yaml = @"
tokenType: azureTrusted
azureTrusted:
  tenantId: $($env:AZURE_TENANT_ID | ConvertTo-Json)
  clientId: $($env:AZURE_CLIENT_ID | ConvertTo-Json)
  clientSecret: $($env:AZURE_CLIENT_SECRET | ConvertTo-Json)
  region: $($region | ConvertTo-Json)
  account: $($env:AZURE_SIGNING_ACCOUNT | ConvertTo-Json)
  profile: $($env:AZURE_CERT_PROFILE | ConvertTo-Json)
msTimestampUrl: $($ts | ConvertTo-Json)
"@
  Set-Content -Path $cfg -Value $yaml -Encoding utf8
  Write-Output "Signing with OSSign Azure Trusted Signing ($env:AZURE_SIGNING_ACCOUNT / $env:AZURE_CERT_PROFILE): $($Paths -join ', ')"
  foreach ($path in $Paths) {
    $type = Get-OsSignType $path
    & $ossign -c $cfg -t $type -o $path $path
    if ($LASTEXITCODE -ne 0) { throw "ossign failed on $path (exit $LASTEXITCODE)" }
  }
}

$Files = $Files | ForEach-Object { (Resolve-Path $_).Path }
$haveCert = [bool]$env:WINDOWS_CERTIFICATE
# Same core Azure names as suite sign.ps1; endpoint OR region is enough.
$azureCore = 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERT_PROFILE'
$haveAzureCore = -not ($azureCore | Where-Object { -not [Environment]::GetEnvironmentVariable($_) })
$haveAzureEndpoint = [bool]$env:AZURE_SIGNING_ENDPOINT -or [bool]$env:AZURE_SIGNING_REGION
$haveAzure = $haveAzureCore -and $haveAzureEndpoint
$selfFlag = "$env:SIGN_SELF_SIGNED".Trim().ToLowerInvariant()
$haveSelf = $selfFlag -in @('1', 'true', 'yes')

if (-not $haveCert -and -not $haveAzure -and -not $haveSelf) {
  Write-Warn "Windows signing secrets not set (WINDOWS_CERTIFICATE, AZURE_*, or SIGN_SELF_SIGNED=1): leaving unsigned: $($Files -join ', ')"
  exit 0
}

$script:Tmp = Join-Path ([IO.Path]::GetTempPath()) "aad-sign-$PID"
New-Item -ItemType Directory -Force -Path $script:Tmp | Out-Null

try {
  if ($haveCert) {
    $pfx = Join-Path $script:Tmp 'cert.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    Invoke-SignWithPfx $pfx $env:WINDOWS_CERTIFICATE_PASSWORD $Files 'WINDOWS_CERTIFICATE (signtool)'
  }
  elseif ($haveAzure) {
    $backend = if ($env:SIGN_BACKEND) { $env:SIGN_BACKEND.Trim().ToLowerInvariant() } else { '' }
    $ossignPath = Find-OsSign
    $useOsSign = ($backend -eq 'ossign') -or (($backend -ne 'signtool') -and [bool]$ossignPath)
    if ($useOsSign) {
      if (-not $ossignPath) { throw 'SIGN_BACKEND=ossign but ossign.exe was not found (set OSSIGN)' }
      Invoke-OsSignAzure $Files
      Assert-HasSignature $Files
      try {
        $script:SignTool = Find-SignTool
        Invoke-SignTool (@('verify', '/pa', '/v') + $Files)
      }
      catch {
        Write-Warn "Signed with OSSign; signtool /pa verify skipped: $($_.Exception.Message)"
      }
    }
    else {
      $script:SignTool = Find-SignTool
      $common = @('sign', '/v', '/fd', 'SHA256', '/td', 'SHA256', '/d', 'MynaCAD', '/du', 'https://github.com/HolagundiWorks/O-DRAFT')
      Write-Output "Signing with Azure Trusted Signing via signtool ($env:AZURE_SIGNING_ACCOUNT / $env:AZURE_CERT_PROFILE): $($Files -join ', ')"
      $pkg = Join-Path $script:Tmp 'trusted-signing.zip'
      Invoke-WebRequest -Uri 'https://www.nuget.org/api/v2/package/Microsoft.Trusted.Signing.Client' -OutFile $pkg
      Expand-Archive -Path $pkg -DestinationPath (Join-Path $script:Tmp 'client') -Force
      $dlib = Get-ChildItem -Path (Join-Path $script:Tmp 'client') -Recurse -Filter Azure.CodeSigning.Dlib.dll |
        Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
      if (-not $dlib) { throw 'Azure.CodeSigning.Dlib.dll not found in Microsoft.Trusted.Signing.Client' }
      $metadata = Join-Path $script:Tmp 'metadata.json'
      $endpoint = $env:AZURE_SIGNING_ENDPOINT
      if (-not $endpoint) {
        $region = Get-AzureSigningRegion
        $endpoint = "https://$region.codesigning.azure.net"
      }
      @{
        Endpoint               = $endpoint
        CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT
        CertificateProfileName = $env:AZURE_CERT_PROFILE
      } | ConvertTo-Json | Set-Content -Path $metadata -Encoding utf8
      $ts = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.acs.microsoft.com' }
      Invoke-SignTool ($common + @('/tr', $ts, '/dlib', $dlib.FullName, '/dmdf', $metadata) + $Files)
      Invoke-SignTool (@('verify', '/pa', '/v') + $Files)
    }
  }
  else {
    $dev = Ensure-SelfSignedPfx
    Invoke-SignWithPfx $dev.Path $dev.Password $Files 'self-signed development certificate'
  }
}
finally {
  Remove-Item -Recurse -Force $script:Tmp -ErrorAction SilentlyContinue
}
