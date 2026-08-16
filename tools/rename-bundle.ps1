# Renames Tauri build artifacts to the canonical cross-platform naming scheme:
#   {productName}-{version}-{os}-{arch}.{ext}
#
#   Windows : DeepSeek-Harness-0.1.0-windows-x64.exe  / .msi
#   Linux   : DeepSeek-Harness-0.1.0-linux-amd64.deb  / -linux-arm64.rpm
#   macOS   : DeepSeek-Harness-0.1.0-macos-x64.dmg    / .app (zip for portable)
#
# Works on Windows / macOS / Linux runners. Tauri emits bundles under
# target/{triple}/release/bundle/... (cross-compile) or target/release/bundle/...
# (native). This script discovers every bundle dir, reads version +
# productName dynamically from tauri.conf.json, and rewrites each installer
# to a hyphen-separated, os/arch-explicit name.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$srcTauri = Join-Path $root 'src-tauri'
$targetDir = Join-Path $srcTauri 'target'

# ---- read version + productName dynamically from tauri.conf.json ----
# (sync-version.ps1 already mirrored package.json's version here beforehand)
$conf = Get-Content (Join-Path $srcTauri 'tauri.conf.json') -Raw | ConvertFrom-Json
$version = $conf.version
$productName = $conf.productName
$name = $productName -replace '\s+', '-'   # "DeepSeek Harness" -> "DeepSeek-Harness"

# Map a Tauri bundle file name to an (os, arch) pair. Tauri uses different
# conventions per platform, so we inspect the original name.
function Get-OsArch([string]$fileName) {
  $os = $null; $arch = $null
  if ($fileName -match '\.(exe|msi)$' -or $fileName -match 'windows|win32') {
    $os = 'windows'
  } elseif ($fileName -match '\.(dmg|app)$' -or $fileName -match 'macos|darwin') {
    $os = 'macos'
  } elseif ($fileName -match '\.(deb|rpm|AppImage)$' -or $fileName -match 'linux') {
    $os = 'linux'
  }

  if ($fileName -match 'aarch64|arm64') { $arch = 'arm64' }
  elseif ($fileName -match 'x86_64|amd64') { $arch = 'amd64' }
  elseif ($fileName -match '\b(x64)\b') { $arch = 'x64' }

  # fallback: infer arch from the build target triple folder if present
  return $os, $arch
}

# File extensions we treat as installer artifacts, per platform.
$artifactExts = @('.exe', '.msi', '.deb', '.rpm', '.dmg', '.app', '.AppImage')

# ---- locate every bundle directory (cross-compile aware) ----
$bundleDirs = @()
if (Test-Path $targetDir) {
  # native build: target/release/bundle
  $native = Join-Path $targetDir 'release' 'bundle'
  if (Test-Path $native) { $bundleDirs += $native }
  # cross builds: target/<triple>/release/bundle
  Get-ChildItem $targetDir -Directory | ForEach-Object {
    $b = Join-Path $_.FullName 'release' 'bundle'
    if (Test-Path $b) { $bundleDirs += $b }
  }
}

if ($bundleDirs.Count -eq 0) {
  Write-Host "no bundle directories found under $targetDir (did the build run?)"
  exit 0
}

$found = 0
foreach ($bundleDir in ($bundleDirs | Select-Object -Unique)) {
  Write-Host "scanning $bundleDir"
  Get-ChildItem $bundleDir -Recurse -File -ErrorAction SilentlyContinue | Where-Object {
    $artifactExts -contains $_.Extension
  } | ForEach-Object {
    $file = $_
    $ext = $file.Extension.TrimStart('.')

    # Determine os/arch. Prefer the original file name; if arch is unknown,
    # fall back to the parent target triple folder name.
    $os, $arch = Get-OsArch $file.Name
    if (-not $arch) {
      # e.g. target/x86_64-unknown-linux-gnu/release/bundle -> triple
      $triple = ($file.DirectoryName -split 'release[/\\]bundle')[0] -split '[/\\]' | Select-Object -Last 1
      if ($triple -match 'aarch64') { $arch = 'arm64' }
      elseif ($triple -match 'x86_64') { $arch = 'amd64' }
      elseif ($triple -match 'x64') { $arch = 'x64' }
    }
    if (-not $os) {
      if ($ext -in 'exe','msi') { $os = 'windows' }
      elseif ($ext -in 'dmg','app') { $os = 'macos' }
      elseif ($ext -in 'deb','rpm','AppImage') { $os = 'linux' }
    }
    if (-not $arch) { $arch = 'x64' }
    if (-not $os) { $os = 'unknown' }

    # .app is a directory-bundle; ship it as a .app.zip for a clean artifact name
    if ($ext -eq 'app') {
      $zipName = "{0}-{1}-{2}-{3}.app.zip" -f $name, $version, $os, $arch
      $zipPath = Join-Path $file.DirectoryName $zipName
      if (-not (Test-Path $zipPath)) {
        Compress-Archive -Path $file.FullName -DestinationPath $zipPath -Force
        Write-Host "zipped -> $zipName"
      }
      $found++
      return
    }

    $newName = "{0}-{1}-{2}-{3}.{4}" -f $name, $version, $os, $arch, $ext
    $dest = Join-Path $file.DirectoryName $newName
    if ($file.Name -ne $newName) {
      if (Test-Path $dest) { Remove-Item $dest -Force }
      Move-Item $file.FullName $dest -Force
      Write-Host "renamed -> $newName"
    } else {
      Write-Host "skip (already named) $newName"
    }
    $found++
  }
}

Write-Host "renamed $found artifact(s)"
