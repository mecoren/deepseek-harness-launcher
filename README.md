# DeepSeek Harness 桌面启动器

基于 **Tauri 2** 的轻量桌面外壳，把 DeepSeek 网页工作台（`dsh web`）封装成一个
原生 Windows 应用：系统托盘常驻、自带标题栏、内置离线运行环境，终端用户无需安装
Node.js / npm 即可使用与更新。

---

## 为什么要有这个项目

DeepSeek 官方提供的 `dsh web` 是一个命令行启动的本地网页工作台，但它本质上是
“跑在终端里的一条命令”，存在几个对普通用户不友好的地方：

- **没有原生窗口外壳**：`dsh web` 启动后只往终端打印一个 `http://127.0.0.1:<port>`
  地址，用户得自己开浏览器访问；没有最小化/关闭、托盘驻留、独立窗口这些桌面体验。
- **依赖用户本机的 Node / npm 环境**：跑 `dsh web` 需要先装好 Node.js，更新时还要
  用 `npm`，普通用户机器上往往没有，或者版本不对。
- **更新链路裸露**：检查版本、拉新版都散落在命令行里，没有统一入口。

这个项目就是把这些“零散的命令行体验”包成一个**开箱即用的桌面应用**：

- 用 Tauri 2 提供一个原生窗口（自定义注入标题栏，去掉了系统边框）；
- 启动后自动在后台拉起 `dsh web`（loopback 随机端口），主窗口直接导航过去；
- 把 Node 运行环境和 `dsh` CLI **打包进应用内部**（`runtime-host/`），首次运行免联网；
- 更新功能改用应用**自带的 pnpm**，不再调用用户机器上的 npm，所以最终用户也不用装 npm。

一句话：**让 `dsh web` 从“一条命令”变成“一个双击就开的桌面程序”。**

---

## 它做什么

- **桌面外壳**：Tauri 2 窗口承载 DeepSeek 网页工作台，去掉系统边框，
  注入自定义标题栏（`🐋 DeepSeek Harness` 品牌 + 最小化 / 全屏 / 关闭按钮）。
- **自动启动工作台**：后台线程以 `dsh web --host 127.0.0.1 --port 0`
  （系统随机分配 loopback 端口）拉起服务，读取就绪行 `dsh web: <url>` 后把主窗口
  导航过去。点击标题栏品牌文字可展开下拉菜单，内含「检查 DeepSeek Harness 更新」。
- **托盘常驻**：关闭窗口 **只隐藏**，进程继续驻留系统托盘（黑鲸图标）；
  只有托盘菜单的「退出」才真正结束进程，并在退出时强杀 `dsh web` 进程树。
- **离线运行**：优先使用内置 `runtime-host/`（随包发布的 Node 可执行文件 +
  `@deepseek-ai/dsh` CLI + 自带 pnpm），**首次运行完全免联网**。
- **自带更新器**：通过 `runtime-host/node.exe node_modules/pnpm/bin/pnpm.cjs`
  执行 `pnpm info` / `pnpm add` 检查并安装新版，只需能访问 npm registry，
  **不依赖用户机器上的 Node / npm / pnpm**。
- **提示样式**：内联实现 shadcn Sonner 风格的居中 toast（顶部居中、成功绿 /
  失败红、自动消失），不引入任何外部 CDN 或前端组件依赖。

---

## 怎么启动 / 怎么跑起来

### 环境要求（开发者侧，Windows 10 / 11，x64）

| 依赖 | 说明 |
| --- | --- |
| Rust 工具链 | 稳定版（≥ 1.77） |
| MSVC 目标 | `x86_64-pc-windows-msvc`（**不能用 GNU**，Tauri 资源是 MSVC COFF 格式） |
| Visual Studio Build Tools 2022 | “使用 C++ 的桌面开发”工作负载（含 MSVC + Windows SDK） |
| Node.js + npm | ≥ 18，仅用于组装离线 CLI 包；**终端用户不需要** |
| WebView2 运行时 | Win10/11 已预装，Tauri 渲染前端依赖它 |

### 1. 准备 Rust MSVC 目标

```powershell
rustup target add x86_64-pc-windows-msvc
```

### 2. 准备离线 CLI 包（首次必须，否则无离线模式）

`node.exe`、`node_modules`、`pnpm` 已在 `.gitignore` 中忽略，克隆仓库后需要本地重建：

```powershell
cd runtime-host
copy <你的 Node 安装目录>\node.exe node.exe          # 复制一份 Windows Node 进来
npm install @deepseek-ai/dsh --no-audit --no-fund   # 安装官方 CLI
npm install pnpm --save-exact --no-audit --no-fund   # 安装自带更新器
# 保持 .npmrc 的 node-linker=hoisted 不被删掉
```

> 若 `runtime-host/` 缺失，应用会自动回退到 `npx -y @deepseek-ai/dsh web`
> （首次运行需联网下载一次），仍能启动。

### 3. 启动方式

**开发模式（最常用）**

```powershell
cd deepseek-harness-launcher
npm install          # 仅首次：拉取 @tauri-apps/cli
npm run dev          # = tauri dev：编译 debug 二进制并打开窗口
```

> `tauri.conf.json` 的 `devUrl` 为 `null`，`tauri dev` 直接用 Tauri 内置开发服务器
> 伺服静态 `dist/`，**不需要额外的前端 dev server**（本项目没有 Vite/React）。

**只出可执行文件（最快验证）**

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

产物直接双击启动：

```
src-tauri/target/x86_64-pc-windows-msvc/release/deepseek_harness_launcher.exe
```

**打包安装程序**

```powershell
npm run build        # = tauri build（targets: "all"）
```

产物在 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/`（含 NSIS 安装包、便携版等）。

### 4. 启动后行为

1. 外壳先显示加载页（`dist/index.html`，中文 spinner 文案）。
2. 后台拉起 `dsh web`，读取就绪行后主窗口导航到本地工作台。
3. 关闭窗口 → 仅隐藏，进程继续驻留托盘。
4. 托盘菜单「显示主窗口」/「退出」；「退出」才真正结束并强杀 `dsh web` 进程树。

---

## 更多细节

完整的依赖清单、离线包目录结构、pnpm 更新机制、项目结构、排错与 GitHub 元数据，
见仓库内的 [`INSTALL.md`](./INSTALL.md)。

---

## 技术栈

- **Tauri 2**（Rust 后端 + WebView2 渲染）
- 内置 **Node 22** 运行环境 + **@deepseek-ai/dsh** CLI（离线）
- 自带 **pnpm** 更新器
- 自定义注入式标题栏 + 内联 Sonner toast（无前端框架、无外部依赖）
