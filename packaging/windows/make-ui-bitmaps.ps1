<#
.SYNOPSIS
  Regenerate the WiX installer splash bitmaps from the Tren logo.

.DESCRIPTION
  Writes WixUIDialogBmp.bmp (493x312) and WixUIBannerBmp.bmp (493x58) as 24-bit BMPs
  next to this script. Source mark: assets/app-icon/aad-256.png (from `cargo xtask logo`).
#>
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Out = $PSScriptRoot
$LogoPath = Join-Path $Root 'assets\app-icon\aad-256.png'
if (-not (Test-Path $LogoPath)) { throw "logo missing: $LogoPath (run cargo xtask logo)" }

$logo = [System.Drawing.Image]::FromFile($LogoPath)
$blue = [System.Drawing.Color]::FromArgb(255, 23, 24, 27)   # the Tren plate, #17181B
$white = [System.Drawing.Color]::White
$brush = New-Object System.Drawing.SolidBrush $white
$sf = New-Object System.Drawing.StringFormat
$sf.Alignment = [System.Drawing.StringAlignment]::Center

function Save-Bmp24([System.Drawing.Bitmap]$src, [string]$path) {
  $clone = New-Object System.Drawing.Bitmap $src.Width, $src.Height, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
  $g = [System.Drawing.Graphics]::FromImage($clone)
  $g.DrawImage($src, 0, 0, $src.Width, $src.Height)
  $g.Dispose()
  $clone.Save($path, [System.Drawing.Imaging.ImageFormat]::Bmp)
  $clone.Dispose()
}

$dialog = New-Object System.Drawing.Bitmap 493, 312
$g = [System.Drawing.Graphics]::FromImage($dialog)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$g.Clear($blue)
$mark = 128
$x = [int]((493 - $mark) / 2)
$y = [int]((312 - $mark) / 2) - 24
$g.DrawImage($logo, $x, $y, $mark, $mark)
$fontTitle = New-Object System.Drawing.Font 'Segoe UI', 18, ([System.Drawing.FontStyle]::Bold)
$fontSub = New-Object System.Drawing.Font 'Segoe UI', 10
$g.DrawString('Tren', $fontTitle, $brush, (New-Object System.Drawing.RectangleF 0, ($y + $mark + 8), 493, 32), $sf)
$g.DrawString('Architectural drafting with Kivo and Falcon.', $fontSub, $brush, (New-Object System.Drawing.RectangleF 0, ($y + $mark + 40), 493, 24), $sf)
$g.DrawString('Open source  ·  MIT OR Apache-2.0', $fontSub, $brush, (New-Object System.Drawing.RectangleF 0, ($y + $mark + 64), 493, 24), $sf)
Save-Bmp24 $dialog (Join-Path $Out 'WixUIDialogBmp.bmp')
$g.Dispose(); $dialog.Dispose(); $fontTitle.Dispose(); $fontSub.Dispose()

$banner = New-Object System.Drawing.Bitmap 493, 58
$gb = [System.Drawing.Graphics]::FromImage($banner)
$gb.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$gb.Clear($blue)
$gb.DrawImage($logo, 12, 9, 40, 40)
$fontB = New-Object System.Drawing.Font 'Segoe UI', 12, ([System.Drawing.FontStyle]::Bold)
$gb.DrawString('Tren Setup', $fontB, $brush, 60, 18)
Save-Bmp24 $banner (Join-Path $Out 'WixUIBannerBmp.bmp')
$gb.Dispose(); $banner.Dispose(); $fontB.Dispose()

$logo.Dispose(); $brush.Dispose(); $sf.Dispose()
Write-Output "wrote WixUIDialogBmp.bmp and WixUIBannerBmp.bmp"
