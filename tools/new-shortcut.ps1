# 在当前用户桌面创建「DeepSeek Harness Launcher」快捷方式
# 用法（PowerShell）：
#   .\tools\new-shortcut.ps1
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$exe  = Join-Path $root 'src-tauri\target\x86_64-pc-windows-msvc\release\deepseek_harness_launcher.exe'
$icon = Join-Path $root 'src-tauri\icons\icon.ico'

if (-not (Test-Path $exe)) {
    Write-Error "未找到 release 构建: $exe`n请先运行: cargo build --release --target x86_64-pc-windows-msvc"
    exit 1
}

$ws      = New-Object -ComObject WScript.Shell
$desktop = [Environment]::GetFolderPath('Desktop')
$lnkPath = Join-Path $desktop 'DeepSeek Harness Launcher.lnk'
$lnk     = $ws.CreateShortcut($lnkPath)
$lnk.TargetPath       = $exe
$lnk.WorkingDirectory = Split-Path $exe
$lnk.Description       = 'DeepSeek Harness 桌面启动器'
$lnk.WindowStyle       = 1
if (Test-Path $icon) { $lnk.IconLocation = $icon }
$lnk.Save()

Write-Host "已创建桌面快捷方式: $lnkPath"
