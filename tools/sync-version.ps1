# Syncs the version from package.json into src-tauri/tauri.conf.json so the
# bundle version is never maintained in two places.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$pkg = Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
$confPath = Join-Path (Join-Path $root 'src-tauri') 'tauri.conf.json'

$text = Get-Content $confPath -Raw
$text = [regex]::Replace($text, '("version"\s*:\s*")[^"]*(")', { param($m) $m.Groups[1].Value + $pkg.version + $m.Groups[2].Value })
Set-Content $confPath $text -NoNewline
Write-Host ("synced version -> {0}" -f $pkg.version)
