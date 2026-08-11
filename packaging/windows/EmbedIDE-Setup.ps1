# EmbedIDE 2.6.1 Windows setup — installs app AND downloads compilers during setup (not on first launch).
# Run:  Right-click → Run with PowerShell
#   or: powershell -ExecutionPolicy Bypass -File embed-ide-2.6.1-windows-x64-setup.ps1
param(
  [string]$InstallDir = "$env:LOCALAPPDATA\Programs\EmbedIDE",
  [string]$PortableZip = "",
  [string]$SourceDir = ""
)

$ErrorActionPreference = "Stop"
$Version = "2.6.1"
$ReleaseBase = "https://github.com/Kirpich-Space/EmbedIDE/releases/download/v$Version"
$ZipName = "embed-ide-$Version-windows-x64-portable.zip"
$ToolchainDest = Join-Path $env:APPDATA "embed-ide\toolchain"

Write-Host "=============================================="
Write-Host " EmbedIDE $Version — Windows installer"
Write-Host "=============================================="

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $PortableZip) {
  $cand = Join-Path $ScriptDir $ZipName
  if (Test-Path $cand) { $PortableZip = $cand }
}
if (-not $SourceDir) {
  $cand = Join-Path $ScriptDir "win-unpacked"
  if (Test-Path (Join-Path $cand "EmbedIDE.exe")) { $SourceDir = $cand }
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

if ($SourceDir -and (Test-Path (Join-Path $SourceDir "EmbedIDE.exe"))) {
  Write-Host "Copying from $SourceDir …"
  Copy-Item -Path (Join-Path $SourceDir "*") -Destination $InstallDir -Recurse -Force
} else {
  if (-not $PortableZip -or -not (Test-Path $PortableZip)) {
    $PortableZip = Join-Path $env:TEMP $ZipName
    Write-Host "Downloading portable package…"
    Invoke-WebRequest -Uri "$ReleaseBase/$ZipName" -OutFile $PortableZip -UseBasicParsing
  }
  Write-Host "Extracting $PortableZip …"
  $tmp = Join-Path $env:TEMP "embedide-extract-$Version"
  if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
  Expand-Archive -LiteralPath $PortableZip -DestinationPath $tmp -Force
  $root = Get-ChildItem $tmp -Directory | Where-Object { Test-Path (Join-Path $_.FullName "EmbedIDE.exe") } | Select-Object -First 1
  if (-not $root) {
    $root = Get-ChildItem $tmp -Recurse -Filter "EmbedIDE.exe" | Select-Object -First 1
    if ($root) { $root = $root.Directory }
  }
  if (-not $root) { throw "EmbedIDE.exe not found in archive" }
  Copy-Item -Path (Join-Path $root.FullName "*") -Destination $InstallDir -Recurse -Force
}

$exe = Join-Path $InstallDir "EmbedIDE.exe"
if (-not (Test-Path $exe)) { throw "Install failed: $exe missing" }

# Shortcuts
$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\EmbedIDE"
New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
$WshShell = New-Object -ComObject WScript.Shell
$lnk = $WshShell.CreateShortcut((Join-Path $startMenu "EmbedIDE.lnk"))
$lnk.TargetPath = $exe
$lnk.WorkingDirectory = $InstallDir
$lnk.Save()
$desk = Join-Path ([Environment]::GetFolderPath("Desktop")) "EmbedIDE.lnk"
$lnk2 = $WshShell.CreateShortcut($desk)
$lnk2.TargetPath = $exe
$lnk2.WorkingDirectory = $InstallDir
$lnk2.Save()

# Download compilers DURING setup
$ps1 = Join-Path $InstallDir "resources\install-toolchain.ps1"
Write-Host ""
Write-Host "Downloading compilers into $ToolchainDest …"
Write-Host "(ARM GCC, OpenOCD, Zig, Rust — may take several minutes)"
if (Test-Path $ps1) {
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $ps1 -Dest $ToolchainDest
} else {
  Write-Warning "install-toolchain.ps1 missing — use Settings → Toolchain after launch"
}

Write-Host ""
Write-Host "Done."
Write-Host "  App:       $exe"
Write-Host "  Toolchain: $ToolchainDest"
Write-Host "Launch EmbedIDE from the Start Menu or Desktop shortcut."
