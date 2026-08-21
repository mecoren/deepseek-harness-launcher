#!/usr/bin/env node
// 构建前 fail-fast 校验：runtime-host 离线包必须自带 node、离线 dsh CLI 与独立 npm 更新器。
//
// 背景：node.exe / node_modules / tools 均被 .gitignore 忽略（仓库只提交元数据），
// 全新克隆后若忘记执行 tools/prepare-runtime-host.ps1 就直接 `npm run build`，
// tauri build 会静默打出一个「检查更新必失败、离线启动回退 npx」的残缺安装包，
// 最终用户机器上无 Node/npm 时应用直接卡死。本脚本在打包前强制拦截，杜绝该类
// 残缺产物流出（本次修复的「检查更新失败：runtime-host 内未找到自带的 npm」
// 即由此产生）。
//
// 路径一律基于脚本自身位置解析，不依赖 tauri 钩子的 cwd。

import { existsSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const rh = join(repoRoot, 'runtime-host');
const problems = [];

// 1. 真实 Node 二进制：Windows 为 node.exe，其他平台为 node。
//    体积须 ≥ 20MB——nvmd/nvm 的 bin 目录里只有几 MB 的版本管理 shim，
//    复制进离线包后运行会报 0xC0000135（DLL 缺失）。
const nodeBin = join(rh, process.platform === 'win32' ? 'node.exe' : 'node');
if (!existsSync(nodeBin)) {
  problems.push(
    `缺少 ${join('runtime-host', process.platform === 'win32' ? 'node.exe' : 'node')}（离线启动会回退 npx，最终用户无 Node 时应用卡死在加载页）`,
  );
} else if (statSync(nodeBin).size < 20 * 1024 * 1024) {
  problems.push(
    `runtime-host 内的 Node 二进制仅 ${Math.round(statSync(nodeBin).size / 1024 / 1024)}MB，` +
      '疑似 nvmd/nvm 的 shim（真实 Node 约 80+MB），运行会报 0xC0000135',
  );
}

// 2. 独立 npm 更新器：必须位于 tools/npm（@deepseek-ai/dsh 依赖图之外），
//    `npm install` 更新 dsh 时才不会重写更新器自身。
const npmCli = join(rh, 'tools', 'npm', 'node_modules', 'npm', 'bin', 'npm-cli.js');
if (!existsSync(npmCli)) {
  problems.push(
    '缺少独立 npm 更新器（runtime-host/tools/npm/node_modules/npm/bin/npm-cli.js），' +
      '安装包内「检查更新 / 更新」功能必然失败',
  );
}

// 3. 离线 dsh CLI：spawn_dsh_web 期望的扁平布局入口。
const dshBin = join(rh, 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js');
if (!existsSync(dshBin)) {
  problems.push(
    '缺少离线 dsh CLI（runtime-host/node_modules/@deepseek-ai/dsh/lib/bin.js），' +
      '离线启动会回退 npx',
  );
}

if (problems.length > 0) {
  console.error('\n[x] runtime-host 离线包不完整，已中止打包：');
  for (const p of problems) console.error('    - ' + p);
  console.error('');
  console.error('    修复方法：在仓库根目录执行  .\\tools\\prepare-runtime-host.ps1');
  console.error('    重新生成离线包后再次构建（详见 INSTALL.md 第 5 节）。\n');
  process.exit(1);
}

console.log('[ok] runtime-host 离线包校验通过（Node 二进制 + dsh CLI + 独立 npm 更新器齐全）');
