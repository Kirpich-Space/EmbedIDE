# EmbedIDE Windows toolchain installer — run by NSIS during setup.
param(
  [Parameter(Mandatory = $true)][string]$Dest
)

$ErrorActionPreference = "Stop"
$Cache = Join-Path $env:LOCALAPPDATA "embed-ide\toolchain-cache"
New-Item -ItemType Directory -Force -Path $Dest, $Cache | Out-Null
$Bin = Join-Path $Dest "bin"
New-Item -ItemType Directory -Force -Path $Bin | Out-Null

function Download($Url, $Out) {
  Write-Host "  ↓ $Url"
  Invoke-WebRequest -Uri $Url -OutFile $Out -UseBasicParsing
}

function ExtractZip($Zip, $DestDir) {
  if (Test-Path $DestDir) { Remove-Item -Recurse -Force $DestDir }
  New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
  Expand-Archive -LiteralPath $Zip -DestinationPath $DestDir -Force
  $kids = Get-ChildItem $DestDir | Where-Object { $_.Name -notlike ".*" }
  if ($kids.Count -eq 1 -and $kids[0].PSIsContainer) {
    $inner = $kids[0].FullName
    Get-ChildItem $inner | ForEach-Object {
      Move-Item $_.FullName (Join-Path $DestDir $_.Name) -Force
    }
    Remove-Item $inner -Recurse -Force
  }
}

Write-Host "EmbedIDE: installing toolchain to $Dest"

$pkgs = @(
  @{ Name = "gcc"; Url = "https://github.com/xpack-dev-tools/arm-none-eabi-gcc-xpack/releases/download/v14.2.1-1.1/xpack-arm-none-eabi-gcc-14.2.1-1.1-win32-x64.zip" },
  @{ Name = "openocd"; Url = "https://github.com/xpack-dev-tools/openocd-xpack/releases/download/v0.12.0-6/xpack-openocd-0.12.0-6-win32-x64.zip" },
  @{ Name = "zig"; Url = "https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.zip"; Zig = $true }
)

foreach ($p in $pkgs) {
  $file = Join-Path $Cache (Split-Path $p.Url -Leaf)
  if (-not (Test-Path $file) -or (Get-Item $file).Length -lt 1000) {
    Download $p.Url $file
  } else {
    Write-Host "  = cache $($p.Name)"
  }
  $unpack = Join-Path $Dest $p.Name
  Write-Host "  ✦ extract $($p.Name)"
  ExtractZip $file $unpack
  if ($p.Zig) {
    $zigBin = Join-Path $unpack "bin"
    New-Item -ItemType Directory -Force -Path $zigBin | Out-Null
    $zigExe = Get-ChildItem -Path $unpack -Filter "zig.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($zigExe) { Copy-Item $zigExe.FullName (Join-Path $zigBin "zig.exe") -Force }
  }
  $pkgBin = Join-Path $unpack "bin"
  if (Test-Path $pkgBin) {
    Get-ChildItem $pkgBin | ForEach-Object {
      Copy-Item $_.FullName (Join-Path $Bin $_.Name) -Force
    }
  }
}

# Rust via rustup-init
$rustRoot = Join-Path $Dest "rust"
$env:RUSTUP_HOME = Join-Path $rustRoot "rustup"
$env:CARGO_HOME = Join-Path $rustRoot "cargo"
New-Item -ItemType Directory -Force -Path $env:RUSTUP_HOME, $env:CARGO_HOME | Out-Null
$rustup = Join-Path $Cache "rustup-init.exe"
if (-not (Test-Path $rustup)) {
  Download "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" $rustup
}
& $rustup -y --no-modify-path --default-toolchain stable `
  -t thumbv6m-none-eabi -t thumbv7m-none-eabi -t thumbv7em-none-eabi -t thumbv7em-none-eabihf

foreach ($tool in @("cargo.exe", "rustc.exe", "rustup.exe")) {
  $src = Join-Path $env:CARGO_HOME "bin\$tool"
  if (Test-Path $src) { Copy-Item $src (Join-Path $Bin $tool) -Force }
}

@{
  platform = "win-x64"
  fetchedAt = (Get-Date).ToUniversalTime().ToString("o")
  source = "nsis-installer"
  rust = $true
} | ConvertTo-Json | Set-Content -Path (Join-Path $Dest "manifest.json") -Encoding UTF8

Write-Host "Done: $Dest"
