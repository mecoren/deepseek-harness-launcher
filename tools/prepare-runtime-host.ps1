# tools/prepare-runtime-host.ps1
#
# 一键重建 runtime-host 离线包（真实 node.exe + 扁平 node_modules）。
#
# 为什么需要这个脚本：
#   1. node.exe / node_modules 被 .gitignore 忽略，克隆后需要本地重建；
#   2. 必须使用 **npm**（扁平、无符号链接）安装依赖——若用 pnpm 默认的
#      symlink 布局（`.pnpm` 虚拟 store + 顶层符号链接），Tauri 打包复制
#      resources 时符号链接会丢失，导致安装后
#      `node_modules/pnpm`、`node_modules/@deepseek-ai/dsh` 缺失，
#      更新功能与离线启动同时失效（见 INSTALL.md 排错）；
#   3. nvmd / nvm 的 bin 目录里的 `node.exe` 是几 MB 的版本管理 shim，
#      不是真实 Node（真实 Node 约 80+ MB），单独复制后运行会报
#      `0xC0000135`（DLL 缺失）。脚本会校验并自动从 nvmd 的 versions
#      目录挑选 ≥ 22.16 的最新版本；
#   4. npm 11 默认拦截依赖的 install 脚本（allow-scripts 机制），脚本会
#      `npm approve-scripts --allow-scripts-pending` 批准并 `npm rebuild`
#      生成 koffi / node-pty / dsh-subprocess-local 等原生模块。
#
# 用法（在仓库根目录执行，PowerShell 7+）：
#   .\tools\prepare-runtime-host.ps1
#   也可显式指定真实 node.exe：
#   .\tools\prepare-runtime-host.ps1 -NodeExe "C:\Program Files\nodejs\node.exe"
#
# 完成后执行 `npm run build` 重新打包。

param(
    [string]$NodeExe = ""
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$runtimeHost = Join-Path $repoRoot 'runtime-host'

if (-not (Test-Path (Join-Path $runtimeHost 'package.json'))) {
    Write-Host "未找到 runtime-host/package.json，请确认在仓库根目录下执行本脚本。" -ForegroundColor Red
    exit 1
}

# 真实 node.exe 需 ≥ 20MB（nvmd/nvm 的 bin shim 只有几 MB）。
function Test-RealNode([string]$Path) {
    return $Path -and (Test-Path $Path) -and (Get-Item $Path).Length -gt 20MB
}

# 选择真实 node.exe：显式指定 > PATH > nvmd versions 目录自动探测
$node = $null
if (Test-RealNode $NodeExe) { $node = $NodeExe }
if (-not $node) { $cand = (Get-Command node -ErrorAction SilentlyContinue).Source; if (Test-RealNode $cand) { $node = $cand } }
if (-not $node) {
    $nvmdVers = Join-Path $env:USERPROFILE '.nvmd\versions'
    if (Test-Path $nvmdVers) {
        $cands = Get-ChildItem $nvmdVers -Directory -ErrorAction SilentlyContinue |
            ForEach-Object {
                if ($_.Name -match '^(\d+)\.(\d+)\.(\d+)$') {
                    [PSCustomObject]@{ Maj = [int]$Matches[1]; Min = [int]$Matches[2]; Patch = [int]$Matches[3]; Dir = $_.FullName }
                }
            } |
            Where-Object { $_.Maj -gt 22 -or ($_.Maj -eq 22 -and $_.Min -ge 16) } |
            Sort-Object Maj, Min, Patch -Descending
        # 优先 v22 LTS（项目文档指定 v22 ≥ 22.16，见 INSTALL.md），其次更高主版本
        $best = @($cands | Where-Object { $_.Maj -eq 22 }) + @($cands | Where-Object { $_.Maj -gt 22 }) |
            Select-Object -First 1
        if ($best) {
            $cand = Join-Path $best.Dir 'node.exe'
            if (Test-RealNode $cand) { $node = $cand }
        }
    }
}
if (-not $node) {
    Write-Host "找不到真实 node.exe（≥ 20MB）。请通过 -NodeExe 指定，例如：" -ForegroundColor Yellow
    Write-Host "  .\tools\prepare-runtime-host.ps1 -NodeExe 'C:\Program Files\nodejs\node.exe'"
    exit 1
}

Write-Host "==> 清理旧离线包（删除损坏的符号链接布局）" -ForegroundColor Cyan
foreach ($p in @('node_modules', 'node.exe', 'pnpm-lock.yaml')) {
    $target = Join-Path $runtimeHost $p
    if (Test-Path $target) {
        try {
            if ((Get-Item $target -Force).PSIsContainer) {
                # PowerShell 7.5+ 的 safe-delete 会拦截大目录删除，改用 .NET API
                [System.IO.Directory]::Delete($target, $true)
            } else {
                Remove-Item $target -Force
            }
            Write-Host "    已删除 $p"
        } catch {
            Write-Host "    [WARN] 删除 $p 失败：$($_.Exception.Message)" -ForegroundColor Yellow
        }
    }
}

Write-Host "==> 复制真实 node.exe（$node）" -ForegroundColor Cyan
Copy-Item $node (Join-Path $runtimeHost 'node.exe')

Write-Host "==> npm install（扁平布局，无符号链接）" -ForegroundColor Cyan
Push-Location $runtimeHost
try {
    npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw "npm install 失败（exit=$LASTEXITCODE）" }

    # npm 11 默认拦截依赖的 install 脚本：批准并重建原生模块
    Write-Host "==> 批准依赖 install 脚本并重建原生模块" -ForegroundColor Cyan
    $approveOut = npm approve-scripts --allow-scripts-pending 2>&1
    if ($LASTEXITCODE -eq 0) {
        npm rebuild 2>&1 | Out-Host
        if ($LASTEXITCODE -ne 0) {
            Write-Host "    [WARN] npm rebuild 未完全成功，原生模块可能不完整（koffi/node-pty）" -ForegroundColor Yellow
        }
    } else {
        Write-Host "    npm 不支持 approve-scripts（旧版本），跳过原生模块重建" -ForegroundColor Yellow
    }
} finally {
    Pop-Location
}

Write-Host "==> 校验离线包" -ForegroundColor Cyan
$node = Join-Path $runtimeHost 'node.exe'
$cli  = Join-Path $runtimeHost 'node_modules\@deepseek-ai\dsh\lib\bin.js'
$pnpm = Join-Path $runtimeHost 'node_modules\pnpm\bin\pnpm.mjs'
if (-not (Test-Path $pnpm)) { $pnpm = Join-Path $runtimeHost 'node_modules\pnpm\bin\pnpm.cjs' }

if (Test-Path $cli) {
    Write-Host -NoNewline "    [OK] @deepseek-ai/dsh 离线 CLI 版本: "
    & $node $cli --version
} else {
    Write-Host "    [WARN] node_modules/@deepseek-ai/dsh/lib/bin.js 未找到，离线启动将回退 npx" -ForegroundColor Yellow
}

if (Test-Path $pnpm) {
    Write-Host -NoNewline "    [OK] pnpm 版本: "
    & $node $pnpm --version
} else {
    Write-Host "    [WARN] node_modules/pnpm/bin/ 下未找到 pnpm 入口（pnpm.mjs / pnpm.cjs）" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "离线包准备完成。接下来请执行 npm run build 重新打包安装。" -ForegroundColor Green
