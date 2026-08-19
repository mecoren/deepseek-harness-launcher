# Renames Tauri build artifacts to the canonical cross-platform naming scheme:
#   {productName}-{version}-{OS}-{Arch}[-{Variant}].{ext}
#
#   Windows : DeepSeek-Harness-Launcher-0.1.0-Windows-Amd64-Installer.msi
#             DeepSeek-Harness-Launcher-0.1.0-Windows-Amd64-Portable.exe
#             DeepSeek-Harness-Launcher-0.1.0-Windows-Amd64-Portable.zip
#   Linux   : DeepSeek-Harness-Launcher-0.1.0-Linux-Amd64.deb
#             DeepSeek-Harness-Launcher-0.1.0-Linux-Amd64.rpm
#             DeepSeek-Harness-Launcher-0.1.0-Linux-Amd64.tar.gz
#             DeepSeek-Harness-Launcher-0.1.0-Linux-Amd64-WebKit41.tar.gz
#   macOS   : DeepSeek-Harness-Launcher-0.1.0-MacOS-Amd64.dmg
#             DeepSeek-Harness-Launcher-0.1.0-MacOS-Amd64.app.zip
#
# Works on Windows / macOS / Linux runners. Tauri emits bundles under
# target/{triple}/release/bundle/... (cross-compile) or target/release/bundle/...
# (native). This script discovers every bundle dir, reads version + productName
# dynamically from tauri.conf.json, and rewrites each installer to a hyphenated,
# OS/arch/variant-explicit name matching the GitHub Release asset style.

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

# Map a raw Tauri bundle file name to a canonical (OS, Arch) pair.
function Get-OsArch([string]$fileName, [string]$tripleHint) {
  $os = $null; $arch = $null

  if ($fileName -match '\.(exe|msi)$' -or $fileName -match 'windows|win32') {
    $os = 'Windows'
  } elseif ($fileName -match '\.(dmg|app)$' -or $fileName -match 'macos|darwin') {
    $os = 'MacOS'
  } elseif ($fileName -match '\.(deb|rpm|AppImage|tar\.gz)$' -or $fileName -match 'linux') {
    $os = 'Linux'
  }

  if ($fileName -match 'aarch64|arm64') { $arch = 'Arm64' }
  elseif ($fileName -match 'x86_64|amd64') { $arch = 'Amd64' }
  elseif ($fileName -match '\b(x64)\b') { $arch = 'Amd64' }

  # Fallback: infer arch from the target triple folder name.
  if (-not $arch -and $tripleHint) {
    if ($tripleHint -match 'aarch64') { $arch = 'Arm64' }
    elseif ($tripleHint -match 'x86_64') { $arch = 'Amd64' }
    elseif ($tripleHint -match 'x64') { $arch = 'Amd64' }
  }

  # Last-resort fallback based on extension.
  if (-not $os) {
    $ext = [System.IO.Path]::GetExtension($fileName).TrimStart('.').ToLower()
    switch ($ext) {
      'exe' { $os = 'Windows' }
      'msi' { $os = 'Windows' }
      'dmg' { $os = 'MacOS' }
      'app' { $os = 'MacOS' }
      'deb' { $os = 'Linux' }
      'rpm' { $os = 'Linux' }
      'appimage' { $os = 'Linux' }
    }
  }

  if (-not $arch) { $arch = 'Amd64' }
  if (-not $os) { $os = 'Unknown' }

  return $os, $arch
}

# Determine the variant suffix for a given artifact.
function Get-Variant([string]$fileName, [string]$ext) {
  $lower = $fileName.ToLower()
  if ($ext -eq 'msi') { return 'Installer' }
  if ($ext -eq 'exe') {
    if ($lower -match 'setup') { return 'Installer' }
    return 'Portable'
  }
  if ($ext -eq 'zip' -and $lower -match 'portable') { return 'Portable' }
  return $null
}

# Build the canonical file name.
function Get-CanonicalName([string]$baseName, [string]$version, [string]$os, [string]$arch, [string]$variant, [string]$ext) {
  if ($variant) {
    return "{0}-{1}-{2}-{3}-{4}.{5}" -f $baseName, $version, $os, $arch, $variant, $ext
  }
  return "{0}-{1}-{2}-{3}.{4}" -f $baseName, $version, $os, $arch, $ext
}

# File extensions we treat as shippable installer artifacts (lowercase for case-insensitive matching).
$artifactExts = @('.exe', '.msi', '.deb', '.rpm', '.dmg', '.appimage')

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
    $ext = $file.Extension.TrimStart('.').ToLower()

    # Derive os/arch, preferring the original file name; fall back to triple.
    $triple = ($file.DirectoryName -split 'release[/\\]bundle')[0] -split '[/\\]' | Select-Object -Last 1
    $os, $arch = Get-OsArch $file.Name $triple
    $variant = Get-Variant $file.Name $ext

    $newName = Get-CanonicalName $name $version $os $arch $variant $ext
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

  # ---- package macOS .app bundles as .app.zip ----
  Get-ChildItem $bundleDir -Recurse -Directory -ErrorAction SilentlyContinue | Where-Object {
    $_.Extension -eq '.app'
  } | ForEach-Object {
    $appDir = $_
    $triple = ($appDir.DirectoryName -split 'release[/\\]bundle')[0] -split '[/\\]' | Select-Object -Last 1
    $os, $arch = Get-OsArch $appDir.Name $triple
    $newName = Get-CanonicalName $name $version $os $arch $null 'app.zip'
    $zipPath = Join-Path $appDir.DirectoryName $newName
    if (-not (Test-Path $zipPath)) {
      Compress-Archive -Path $appDir.FullName -DestinationPath $zipPath -Force
      Write-Host "zipped -> $newName"
    } else {
      Write-Host "skip (already exists) $newName"
    }
    $found++
  }
}

# ---- create Linux portable tar.gz archives (binary + runtime-host) ----
# Scan target/<triple>/release/ for Linux binaries and bundle the offline
# runtime-host alongside the executable, producing both a plain and a
# WebKitGTK-4.1 labelled archive to mirror the GitHub Release asset style.
$linuxTriples = @(
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu'
)
$binaryName = 'deepseek_harness_launcher'
$runtimeHost = Join-Path $root 'runtime-host'

foreach ($triple in $linuxTriples) {
  $releaseDir = Join-Path $targetDir $triple 'release'
  $binary = Join-Path $releaseDir $binaryName
  if (-not (Test-Path $binary)) { continue }

  $arch = if ($triple -match 'aarch64') { 'Arm64' } else { 'Amd64' }
  $tmpBase = $env:RUNNER_TEMP
  if (-not $tmpBase) { $tmpBase = Join-Path $releaseDir "_tmp-$triple" }

  # Put tar.gz archives into the bundle directory so tauri-action picks them up.
  $outDir = Join-Path $targetDir $triple 'release' 'bundle'
  if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }

  # Plain archive: just the binary.
  $plainDir = Join-Path $tmpBase "linux-portable-$triple-plain"
  if (Test-Path $plainDir) { Remove-Item $plainDir -Recurse -Force }
  New-Item -ItemType Directory -Path $plainDir -Force | Out-Null
  Copy-Item $binary (Join-Path $plainDir $binaryName) -Force
  if (-not $IsWindows) { chmod +x (Join-Path $plainDir $binaryName) }

  $plainName = Get-CanonicalName $name $version 'Linux' $arch $null 'tar.gz'
  $plainTar = Join-Path $outDir $plainName
  if (Test-Path $plainTar) { Remove-Item $plainTar -Force }
  tar -czf $plainTar -C $plainDir .
  Write-Host "created -> $plainName"
  $found++

  # WebKit41 archive: binary + offline runtime-host.
  $wkDir = Join-Path $tmpBase "linux-portable-$triple-webkit41"
  if (Test-Path $wkDir) { Remove-Item $wkDir -Recurse -Force }
  New-Item -ItemType Directory -Path $wkDir -Force | Out-Null
  Copy-Item $binary (Join-Path $wkDir $binaryName) -Force
  if (-not $IsWindows) { chmod +x (Join-Path $wkDir $binaryName) }
  if (Test-Path $runtimeHost) {
    Copy-Item $runtimeHost (Join-Path $wkDir 'runtime-host') -Recurse -Force
  }

  $wkName = Get-CanonicalName $name $version 'Linux' $arch 'WebKit41' 'tar.gz'
  $wkTar = Join-Path $outDir $wkName
  if (Test-Path $wkTar) { Remove-Item $wkTar -Force }
  tar -czf $wkTar -C $wkDir .
  Write-Host "created -> $wkName"
  $found++

  if (Test-Path $plainDir) { Remove-Item $plainDir -Recurse -Force }
  if (Test-Path $wkDir) { Remove-Item $wkDir -Recurse -Force }
}

Write-Host "renamed/created $found artifact(s)"
