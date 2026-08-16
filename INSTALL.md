# 安装与构建流程（DeepSeek Harness 桌面启动器）

基于 **Tauri 2** 的桌面外壳，用于在本机启动 DeepSeek 网页工作台（`dsh web`），
托盘常驻、关闭窗口只隐藏不退出、退出时主动杀死 `dsh web` 进程树；
内置**离线 `dsh` CLI 包**（`runtime-host/`），首次运行无需联网。

---

## 1. 环境要求（Windows 10 / 11，x64）

| 依赖 | 版本 / 说明 | 用途 |
| --- | --- | --- |
| **Rust 工具链** | 稳定版（建议 ≥ 1.77） | 编译 Tauri 后端 |
| **MSVC 目标** | `x86_64-pc-windows-msvc` | ⚠️ 必须用 MSVC，不能用 GNU（见排错） |
| **Visual Studio Build Tools 2022** | “使用 C++ 的桌面开发”工作负载（含 MSVC + Windows 10/11 SDK） | Tauri 链接 COFF 格式的 `resource.lib` 需要 MSVC `link.exe` |
| **Node.js + npm** | ≥ 18（仅用于组装离线 CLI 包 / `npx` 兜底） | 打包 `runtime-host`；终端用户无需安装 |
| **WebView2 运行时** | Win10/11 已预装 | Tauri 渲染前端（缺则去微软官网装） |
| **@tauri-apps/cli** | `2.11.4`（devDependency） | 打包安装程序 |

> 安装 VS Build Tools 后，`cargo` 在 `msvc` 目标上会通过 `vswhere` 自动找到
> `link.exe`，**不需要**手动执行 `vcvars64.bat`。

---

## 2. 准备 Rust MSVC 目标

```powershell
rustup target add x86_64-pc-windows-msvc
```

---

## 3. 构建

### 方式 A：只出可执行文件（最快验证）

```powershell
cd deepseek-harness-launcher
cargo build --release --target x86_64-pc-windows-msvc
```

产物：

```
src-tauri/target/x86_64-pc-windows-msvc/release/deepseek_harness_launcher.exe
```

直接双击即可启动。

### 方式 A'：开发模式启动（最常用，等价于你问的 `npm tauri dev`）

```powershell
cd deepseek-harness-launcher
npm install                 # 拉取 @tauri-apps/cli（仅首次）
npm run dev                # = tauri dev：编译 debug 二进制并打开窗口
```

> `tauri.conf.json` 已把 `devUrl` 设为 `null`，`tauri dev` 会用 Tauri 内置开发服务器
> 直接伺服静态 `dist/`（加载页），**不需要**额外前端 dev server（本项目没有 Vite/React）。
> `npm run dev` 内部最终也调用 `cargo build`（debug 目标），所以上面的 `cargo build` 只是它的“手动内层版”。

### 方式 B：打包安装程序（NSIS 等）

```powershell
cd deepseek-harness-launcher
npm install                 # 拉取 @tauri-apps/cli
npm run build              # = tauri build（targets: "all"）
```

产物在：

```
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/
```

含 NSIS 安装包（`.exe`）、便携版（`.exe` / `.zip`）等。

> 等价命令：`npm run build:bin` → `tauri build --no-bundle`（只编译，不打安装包）。

---

## 4. 运行 / 启动行为

1. 启动后外壳先显示加载页（`dist/index.html`，中文 spinner 文案）。
2. 后台线程以 `--host 127.0.0.1 --port 0` 拉起 `dsh web`（系统随机分配 loopback 端口），
   读取就绪行 `dsh web: http://127.0.0.1:<port>` 后把主窗口导航过去。
3. 窗口关闭 → **仅隐藏**，进程继续驻留系统托盘（黑鲸图标）。
4. 托盘菜单：**显示主窗口** / **退出**；只有“退出”才真正结束进程，
   并在退出时 `taskkill /T /F` 杀掉 `dsh web` 进程树。

---

## 5. 离线 CLI 包（`runtime-host/`）

默认优先使用内置离线包，**首次运行完全免联网**：

```
runtime-host/
├── node.exe                              # 随包发布的 Node v22（约 87 MB）
├── node_modules/@deepseek-ai/dsh/        # 官方 @deepseek-ai/dsh 包
│   └── lib/bin.js
├── node_modules/pnpm/                    # 自带 pnpm 更新器（更新 dsh 用，不依赖用户机器 npm）
│   └── bin/pnpm.cjs
├── .npmrc                                # node-linker=hoisted：让 pnpm 保持扁平布局
└── package.json                          # 已声明 @deepseek-ai/dsh 与 pnpm 依赖
```

### 如需重新生成 / 刷新离线包

```powershell
cd runtime-host
# 复制一份 Windows Node 可执行文件进来
copy <node-install>\node.exe node.exe
# 安装官方 CLI（仅首次，需要联网）
npm install @deepseek-ai/dsh --no-audit --no-fund
```

> `tauri.conf.json` 中 `bundle.resources.runtime-host` 会把它打进应用资源目录；
> 运行时 `dsh.rs` 通过 `app.path().resource_dir()` 定位。

### 无离线包时的兜底

若 `runtime-host/` 缺失，自动回退到 `npx -y @deepseek-ai/dsh web`
（首次运行需联网下载一次）。

### 更新走自带 pnpm（不依赖用户机器 npm）

「检查 DeepSeek Harness 更新 / 更新」功能用 `runtime-host` 内自带的
`node.exe node_modules/pnpm/bin/pnpm.cjs` 执行，**不再调用用户机器 PATH 上的
npm**。因此最终用户**无需安装 Node.js / npm / pnpm** 即可更新，只要能联网访问
npm registry 拉取新版本即可：

- 查最新版：`pnpm info @deepseek-ai/dsh version`
- 安装更新：`pnpm add @deepseek-ai/dsh --save-exact`（写入 `package.json`）
- `runtime-host/.npmrc` 的 `node-linker=hoisted` 让 pnpm 保持扁平
  `node_modules/@deepseek-ai/dsh` 布局，与 `dsh.rs::spawn_dsh_web` 期望的路径一致
  （pnpm 默认 symlink 布局在打包拷贝后会失效）。
- 重新生成离线包时也要把 pnpm 一起装好：
  `npm install pnpm --save-exact --no-audit --no-fund`，并保持 `.npmrc` 不被删。

---

## 6. 项目结构

```
deepseek-harness-launcher/
├── package.json                 # npm 脚本：dev / build / build:bin
├── dist/index.html              # 加载页（中文文案）
├── icons/                       # 黑鲸图标（顶层，用于文档/说明）
├── runtime-host/                # 离线 dsh CLI 包（见上）
└── src-tauri/
    ├── Cargo.toml               # tauri features = ["tray-icon","image-png","image-ico"]
    ├── Cargo.lock
    ├── tauri.conf.json          # 窗口/图标/资源打包配置
    ├── build.rs
    ├── capabilities/default.json
    ├── icons/                  # Tauri 实际使用的图标（32/128/ico/png）
    └── src/
        ├── main.rs             # 入口：托盘 + 关闭隐藏 + 退出杀进程
        └── dsh.rs             # 启动 dsh web、解析就绪行、按 PID 强杀
```

---

## 7. 排错

**Q: 编译报 `failed to select a version for tauri ... feature menu`**
`menu` 不是有效 feature（桌面端菜单模块常驻）。已在 `Cargo.toml` 改为
`features = ["tray-icon", "image-png", "image-ico"]`，正常不会遇到。

**Q: GNU 目标链接失败 / `resource.lib` COFF 错误**
Tauri 的资源文件是 MSVC COFF 格式，GNU `ld` 无法链接。
**必须用 MSVC 目标**：`cargo build --release --target x86_64-pc-windows-msvc`
（并确保装了 VS Build Tools 2022 的“C++ 桌面开发”工作负载）。

**Q: 启动后白屏 / 一直转圈**
- 确认 Windows 已安装 WebView2 运行时。
- 看终端（或 `target/.../release` 下日志）是否打印 `[dsh] ready in ...s -> http://127.0.0.1:<port>`；
  若无，可能是 `runtime-host/` 缺失且 `npx` 联网失败（首次需联网）。
- `dsh web` 启动有 240s 看门狗超时，超时会被强杀。

**Q: `dsh web` 启动即崩，报 `... exists and is not a symlink; remove it so dsh can manage the installation fallback`**
`dsh web` 启动时会把全局 profile 目录
`C:\Users\<你>\.dsh\profiles\node_modules\@deepseek-ai\*` 修复成**符号链接**，
指向离线包里的同名模块。若此前 `npm install @deepseek-ai/dsh` 被中断，
该目录下可能残留**真实目录**（如 `dsh-tool-ralph`、`dsh-spill-local`），dsh 会因
“期望是符号链接却找到真实目录”而拒绝启动。
修复：删掉（或改名）这个被污染的全局 profile 目录，让 dsh 重新建一份干净的：
```powershell
# 先确认没有 dsh web 在跑，再执行：
mv $env:USERPROFILE\.dsh\profiles $env:USERPROFILE\.dsh\profiles.bak
# 重新启动即可，dsh 会自动重建符号链接
```
（注意：这是用户级全局目录，与本项目无关；干净机器上不会出现。）

**Q: 退出后 `dsh web` 进程残留**
正常退出路径会 `taskkill /T /F`。若异常崩溃未走退出流程，手动：
```powershell
taskkill /IM node.exe /F        # 仅当确认是其进程时
```

---

## 8. 快速清单（清单式复盘）

- [ ] 装 VS Build Tools 2022（C++ 桌面开发）
- [ ] `rustup target add x86_64-pc-windows-msvc`
- [ ] `runtime-host/node.exe` + `runtime-host/node_modules/@deepseek-ai/dsh/lib/bin.js` 就位
- [ ] `runtime-host/node_modules/pnpm/bin/pnpm.cjs` 就位（更新走自带 pnpm）
- [ ] `runtime-host/.npmrc` 含 `node-linker=hoisted`
- [ ] `cargo build --release --target x86_64-pc-windows-msvc`
- [ ] 双击 `.../release/deepseek_harness_launcher.exe` 验证托盘 + 导航

---

## 9. GitHub 仓库元数据

推送到 GitHub 后，建议设置以下 **Topics（标签）**（与上游 `deepseek-ai/deepseek-harness` 对齐）：

```
dsh, ai-agents, cordis, dsh-plugin
```

设置方式（二选一）：
- **网页**：仓库 → Settings → Topics → Add topic
- **CLI**：`gh repo edit <owner/repo> --add-topic "dsh" --add-topic "ai-agents" --add-topic "cordis" --add-topic "dsh-plugin"`
