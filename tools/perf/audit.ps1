#Requires -Version 5.1
<#
.SYNOPSIS
  The MynaCAD performance audit: repeatable timings of the installed (or any built) MynaCAD, with load checks, a warm-up, N runs, medians, and a history file.

.DESCRIPTION
  Replaces the loose scripts that lived in C:\Temp\video (perf_open.ps1, the timing parts of cli_regression.ps1).

  Scenarios (-Scenarios cli,open):
    cli   aad-cli on the test drawing: inspect, PNG export, PDF export, DWG to DXF; aad-cli perf 100000.  Wall time, CPU time and peak working set of each process.
    open  MynaCAD (the WinUI app) opening the test drawing with AAD_PERF=1.  Reads the app's own markers from %LOCALAPPDATA%\OSAS\odraft-perf.log:
          window activated, file opened, first display list ready, first geometry frame (the last two are written by the host).  Also peak memory of the app and
          its child processes, and idle CPU after the load.  There is no "settled by CPU" detector: that number was noise.

  Method: pre-flight load check (refuses to run on a busy machine unless -Force), the test file is copied to a local temp folder (never read from OneDrive),
  one discarded warm-up run, then -Runs runs; reports min, median, max and the spread (max/min); if the spread is above -MaxSpread it runs 3 more and flags the result noisy.
  Every run is appended as one JSON line to docs/qa/perf-history.jsonl with the commit, version, machine and load state.  -Compare checks the medians against
  docs/qa/perf-baseline.json (warn above 1.10x, fail above 1.25x); -SaveBaseline writes that file from this run.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\perf\audit.ps1 -File "C:\path\Working Drawing.dwg" -Label after-pdf-fix -Compare
#>
[CmdletBinding()]
param(
    [string] $File = "",
    [string] $Cli = "$env:LOCALAPPDATA\Programs\O-Draft\aad-cli.exe",
    [string] $Exe = "$env:LOCALAPPDATA\Programs\O-Draft\ODraft.WinUi.exe",
    [string] $Label = "run",
    [string[]] $Scenarios = @("cli", "open"),
    [int] $Runs = 5,
    [double] $MaxSpread = 1.3,
    [double] $MaxLoadPct = 15,
    [switch] $Force,
    [switch] $Compare,
    [switch] $SaveBaseline,
    [string] $History = "",
    [string] $Baseline = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $History) { $History = Join-Path $repo "docs\qa\perf-history.jsonl" }
if (-not $Baseline) { $Baseline = Join-Path $repo "docs\qa\perf-baseline.json" }
if (-not $File) { throw "Pass -File <the test drawing>, for example the 26k-entity Working Drawing.dwg." }
if (-not (Test-Path -LiteralPath $File)) { throw "Test file not found: $File" }

# ---------------------------------------------------------------- machine state
function Get-Machine {
    $procA = @{}; Get-Process | ForEach-Object { try { $procA[$_.Id] = $_.TotalProcessorTime.TotalSeconds } catch { } }
    $t0 = Get-Date
    Start-Sleep -Seconds 5
    $dt = ((Get-Date) - $t0).TotalSeconds
    $cores = [Environment]::ProcessorCount
    $top = @()
    $sum = 0.0
    foreach ($p in Get-Process) {
        try {
            if ($procA.ContainsKey($p.Id)) {
                $pct = ($p.TotalProcessorTime.TotalSeconds - $procA[$p.Id]) / $dt / $cores * 100
                $sum += $pct
                $top += [pscustomobject]@{ name = $p.ProcessName; pct = [math]::Round($pct, 1) }
            }
        } catch { }
    }
    $os = Get-CimInstance Win32_OperatingSystem
    $bat = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue
    $scheme = (powercfg /getactivescheme) -replace '^.*:\s*', ''
    [pscustomobject]@{
        cpu_pct   = [math]::Round($sum, 1)
        top       = @($top | Sort-Object pct -Descending | Select-Object -First 5)
        free_ram_mb = [int]($os.FreePhysicalMemory / 1024)
        ac_power  = (-not $bat) -or ($bat.BatteryStatus -eq 2)
        power_scheme = $scheme
        os_build  = $os.BuildNumber
        computer  = $env:COMPUTERNAME
        cores     = $cores
    }
}

$machine = Get-Machine
Write-Host ("Machine load {0} % (limit {1} %), free RAM {2} MB, AC {3}, power scheme: {4}" -f $machine.cpu_pct, $MaxLoadPct, $machine.free_ram_mb, $machine.ac_power, $machine.power_scheme)
Write-Host ("Busiest: " + (($machine.top | ForEach-Object { "$($_.name) $($_.pct)%" }) -join ", "))
if (($machine.cpu_pct -gt $MaxLoadPct -or -not $machine.ac_power) -and -not $Force) {
    throw "The machine is busy or on battery, so the numbers would be noise. Close what is running (see the list above) or pass -Force."
}

$commit = ""; try { $commit = (git -C $repo rev-parse --short=10 HEAD 2>$null) } catch { }
$cliVersion = ""; if (Test-Path -LiteralPath $Cli) { $cliVersion = (& $Cli --version 2>&1 | Out-String).Trim() }
$exeInfo = ""; if (Test-Path -LiteralPath $Exe) { $v = (Get-Item $Exe).VersionInfo; $exeInfo = "$($v.ProductName) $($v.ProductVersion)" }
$work = Join-Path $env:TEMP ("aad-audit-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Path $work | Out-Null
$local = Join-Path $work ([IO.Path]::GetFileName($File))
Copy-Item -LiteralPath $File -Destination $local

# ---------------------------------------------------------------- helpers
function Median([double[]] $x) { $s = @($x | Sort-Object); $n = $s.Count; if ($n -eq 0) { return $null }; if ($n % 2) { return $s[($n - 1) / 2] }; return ($s[$n / 2 - 1] + $s[$n / 2]) / 2 }

# Runs a program, returns wall ms, CPU ms and peak working set (MB) of the process itself.
function Measure-Process([string] $Path, [string[]] $ArgList) {
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Path
    $psi.Arguments = (($ArgList | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } }) -join ' ')
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = [Diagnostics.Process]::Start($psi)
    $outTask = $p.StandardOutput.ReadToEndAsync(); $errTask = $p.StandardError.ReadToEndAsync()
    $peak = 0.0
    while (-not $p.WaitForExit(100)) { try { $p.Refresh(); $peak = [math]::Max($peak, $p.PeakWorkingSet64 / 1MB) } catch { } }
    $sw.Stop()
    $cpu = $p.TotalProcessorTime.TotalMilliseconds
    [pscustomobject]@{ wall_ms = [double]$sw.ElapsedMilliseconds; cpu_ms = [double][math]::Round($cpu); peak_mb = [math]::Round($peak, 0); exit = $p.ExitCode; out = $outTask.Result }
}

function Summarise([string] $Scenario, [string] $Metric, [double[]] $Samples) {
    $min = ($Samples | Measure-Object -Minimum).Minimum; $max = ($Samples | Measure-Object -Maximum).Maximum
    $spread = if ($min -gt 0) { $max / $min } else { 0 }
    [pscustomobject]@{ scenario = $Scenario; metric = $Metric; samples = $Samples; median = [math]::Round((Median $Samples), 1); min = $min; max = $max; spread = [math]::Round($spread, 2); noisy = ($spread -gt $MaxSpread) }
}

$summary = @()

# ---------------------------------------------------------------- scenario: cli
function Run-CliSet {
    $png = Join-Path $work "out.png"; $pdf = Join-Path $work "out.pdf"; $dxf = Join-Path $work "out.dxf"
    $r = [ordered]@{}
    $x = Measure-Process $Cli @("run", $local, "--cmd", "inspect {}"); $r.inspect = $x
    $x = Measure-Process $Cli @("run", $local, "--export", $png); $r.png = $x
    $x = Measure-Process $Cli @("run", $local, "--export", $pdf); $r.pdf = $x
    $x = Measure-Process $Cli @("convert", $local, $dxf); $r.dwg_to_dxf = $x
    $x = Measure-Process $Cli @("perf", "100000"); $r.perf_100000 = $x
    return $r
}
if ($Scenarios -contains "cli") {
    if (-not (Test-Path -LiteralPath $Cli)) { throw "aad-cli not found: $Cli" }
    Write-Host "cli: warm-up" -ForegroundColor Cyan
    [void](Run-CliSet)
    $all = @()
    for ($i = 1; $i -le $Runs; $i++) { Write-Host "cli: run $i of $Runs" -ForegroundColor Cyan; $all += , (Run-CliSet) }
    $names = @($all[0].Keys)
    $spreadHigh = $false
    foreach ($n in $names) { $w = [double[]]@($all | ForEach-Object { $_[$n].wall_ms }); $s = Summarise "cli" "$n.wall_ms" $w; if ($s.noisy) { $spreadHigh = $true } }
    if ($spreadHigh) { Write-Host "cli: spread above $MaxSpread, three more runs" -ForegroundColor Yellow; for ($i = 1; $i -le 3; $i++) { $all += , (Run-CliSet) } }
    foreach ($n in $names) {
        $summary += Summarise "cli" "$n.wall_ms" ([double[]]@($all | ForEach-Object { $_[$n].wall_ms }))
        $summary += Summarise "cli" "$n.cpu_ms" ([double[]]@($all | ForEach-Object { $_[$n].cpu_ms }))
        $summary += Summarise "cli" "$n.peak_mb" ([double[]]@($all | ForEach-Object { $_[$n].peak_mb }))
    }
}

# ---------------------------------------------------------------- scenario: open
$perfLog = Join-Path $env:LOCALAPPDATA "OSAS\odraft-perf.log"
function Run-Open {
    Get-Process ODraft.WinUi -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2
    Remove-Item $perfLog -ErrorAction SilentlyContinue
    $env:AAD_PERF = "1"
    $p = Start-Process $Exe -ArgumentList ('"' + $local + '"') -PassThru -WorkingDirectory (Split-Path $Exe)
    $startMs = ([DateTimeOffset]$p.StartTime).ToUnixTimeMilliseconds()
    $peakTree = 0.0
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $frameSeen = $false
    while ($sw.Elapsed.TotalSeconds -lt 90) {
        Start-Sleep -Milliseconds 250
        $tree = @($p.Id) + @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)" -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessId })
        $ws = 0.0; foreach ($id in $tree) { try { $ws += (Get-Process -Id $id -ErrorAction Stop).WorkingSet64 / 1MB } catch { } }
        $peakTree = [math]::Max($peakTree, $ws)
        if (Test-Path $perfLog) { if ((Get-Content $perfLog -Raw) -match 'first geometry frame') { $frameSeen = $true; break } }
    }
    Start-Sleep -Seconds 3
    $c0 = $p.TotalProcessorTime.TotalSeconds; Start-Sleep -Seconds 6; $p.Refresh()
    $idle = ($p.TotalProcessorTime.TotalSeconds - $c0) / 6 * 100   # percent of one core
    $log = if (Test-Path $perfLog) { Get-Content $perfLog } else { @() }
    $res = [ordered]@{}
    foreach ($line in $log) {
        if ($line -match 'phase (activated|file opened) at (\d+) ms') { $res["$($Matches[1] -replace ' ', '_')_ms"] = [double]$Matches[2] }
        if ($line -match 'host: marker (first list build started|first list ready|first geometry frame)\b.*epoch_ms=(\d+)') { $res[($Matches[1] -replace ' ', '_') + '_ms'] = [double]([long]$Matches[2] - $startMs) }
    }
    $res.peak_tree_mb = [math]::Round($peakTree, 0)
    $res.idle_cpu_pct = [math]::Round($idle, 1)
    $res.frame_seen = $frameSeen
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Remove-Item Env:AAD_PERF -ErrorAction SilentlyContinue
    return $res
}
if ($Scenarios -contains "open") {
    if (-not (Test-Path -LiteralPath $Exe)) { throw "MynaCAD not found: $Exe" }
    Write-Host "open: warm-up" -ForegroundColor Cyan
    [void](Run-Open)
    $all = @()
    for ($i = 1; $i -le $Runs; $i++) { Write-Host "open: run $i of $Runs" -ForegroundColor Cyan; $all += , (Run-Open) }
    $metrics = @($all | ForEach-Object { $_.Keys } | Sort-Object -Unique | Where-Object { $_ -ne 'frame_seen' })
    $missing = @($all | Where-Object { -not $_.frame_seen }).Count
    if ($missing -gt 0) { Write-Host "open: $missing run(s) never logged 'first geometry frame' (is the host built with the markers?)" -ForegroundColor Yellow }
    foreach ($m in $metrics) {
        $vals = [double[]]@($all | Where-Object { $_.Contains($m) } | ForEach-Object { $_[$m] })
        if ($vals.Count -gt 0) { $summary += Summarise "open" $m $vals }
    }
}

# ---------------------------------------------------------------- report, history, baseline
Write-Host ""
$summary | ForEach-Object { "{0,-6} {1,-26} median {2,10}  min {3,9}  max {4,9}  spread {5}{6}" -f $_.scenario, $_.metric, $_.median, $_.min, $_.max, $_.spread, $(if ($_.noisy) { "  NOISY" } else { "" }) } | Write-Host

$stamp = (Get-Date).ToUniversalTime().ToString("o")
New-Item -ItemType Directory -Path (Split-Path $History) -Force | Out-Null
foreach ($s in $summary) {
    $line = [ordered]@{
        time = $stamp; label = $Label; scenario = $s.scenario; metric = $s.metric; samples = $s.samples; median = $s.median; min = $s.min; max = $s.max; spread = $s.spread; noisy = $s.noisy
        commit = $commit; cli = $cliVersion; app = $exeInfo; file = [IO.Path]::GetFileName($File); file_bytes = (Get-Item $File).Length
        machine = $machine.computer; cpu_load_pct = $machine.cpu_pct; free_ram_mb = $machine.free_ram_mb; ac = $machine.ac_power; power = $machine.power_scheme; os_build = $machine.os_build
    }
    Add-Content -LiteralPath $History -Value ($line | ConvertTo-Json -Compress -Depth 4) -Encoding UTF8
}
Write-Host "History: $History"

$exit = 0
if ($Compare) {
    if (-not (Test-Path $Baseline)) { Write-Host "No baseline at $Baseline (run once with -SaveBaseline on a quiet machine)." -ForegroundColor Yellow }
    else {
        $base = Get-Content $Baseline -Raw | ConvertFrom-Json
        foreach ($s in $summary) {
            $key = "$($s.scenario)/$($s.metric)"
            $b = $base.metrics.PSObject.Properties | Where-Object { $_.Name -eq $key } | Select-Object -First 1
            if (-not $b -or [double]$b.Value -le 0) { continue }
            $ratio = $s.median / [double]$b.Value
            $verdict = if ($ratio -gt 1.25) { "FAIL" } elseif ($ratio -gt 1.10) { "WARN" } else { "ok" }
            if ($verdict -eq "FAIL") { $exit = 1 }
            Write-Host ("{0,-4} {1,-34} {2,10} vs baseline {3,10}  x{4:N2}" -f $verdict, $key, $s.median, $b.Value, $ratio)
        }
    }
}
if ($SaveBaseline) {
    $m = [ordered]@{}
    foreach ($s in $summary) { if (-not $s.noisy) { $m["$($s.scenario)/$($s.metric)"] = $s.median } else { Write-Host "Not saved (noisy): $($s.scenario)/$($s.metric)" -ForegroundColor Yellow } }
    $doc = [ordered]@{ saved = $stamp; commit = $commit; machine = $machine.computer; file = [IO.Path]::GetFileName($File); metrics = $m }
    $doc | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $Baseline -Encoding UTF8
    Write-Host "Baseline written: $Baseline"
}
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
exit $exit
