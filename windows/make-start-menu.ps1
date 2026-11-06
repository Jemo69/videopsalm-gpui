# Perez RS — Start Menu shortcut helper (no installer needed).
# Creates/refreshes the per-user Start Menu + Desktop shortcuts pointing at
# the portable exe next to this script's folder. Run after unzipping:
#   powershell -ExecutionPolicy Bypass -File windows\make-start-menu.ps1
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "perez-rs.exe"
if (-not (Test-Path $exe)) {
  $exe = Join-Path $root "dist\perez-rs.exe"
}
if (-not (Test-Path $exe)) {
  Write-Error "perez-rs.exe not found next to the repo root or in dist/. Build it first: cd gpui-app; cargo build --release -p perez-rs"
}

$shell = New-Object -ComObject WScript.Shell
$startDir = Join-Path ([Environment]::GetFolderPath("Programs")) "Perez RS"
New-Item -ItemType Directory -Force -Path $startDir | Out-Null
$lnk = $shell.CreateShortcut((Join-Path $startDir "Perez RS.lnk"))
$lnk.TargetPath = $exe
$lnk.WorkingDirectory = Split-Path -Parent $exe
$lnk.Description = "Perez RS — offline worship presenter"
$lnk.Save()

$desktop = Join-Path ([Environment]::GetFolderPath("Desktop")) "Perez RS.lnk"
$desk = $shell.CreateShortcut($desktop)
$desk.TargetPath = $exe
$desk.WorkingDirectory = Split-Path -Parent $exe
$desk.Description = "Perez RS — offline worship presenter"
$desk.Save()

Write-Host "Shortcuts created for $exe"
