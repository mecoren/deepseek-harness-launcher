//! `dsh web` launcher module.
//!
//! Spawns `dsh web --host 127.0.0.1 --port 0` (loopback, OS-assigned port) and
//! waits for the canonical readiness line `dsh web: <url>` on stdout. This is
//! the same signal the upstream desktop app relies on. Returns `(url, pid)`.
//!
//! Two launch modes:
//!  * Offline (preferred): if the bundled Node binary (`runtime-host/node`,
//!    or `node.exe` on Windows) + the bundled `@deepseek-ai/dsh` CLI exist
//!    next to the binary, run them directly — no network required at runtime.
//!  * Fallback: otherwise use `npx -y @deepseek-ai/dsh web` (needs Node + a
//!    one-time package download on first run).
//!
//! The caller kills the process by pid on exit.

use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Maximum seconds to wait for the readiness line before giving up.
const READY_TIMEOUT_SECS: u64 = 240;

/// File name of the bundled Node binary inside `runtime-host`, per platform.
///
/// Windows ships `node.exe`; every other OS ships `node`.
fn node_bin_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

/// Strip the Windows verbatim `\\?\` prefix from a path.
///
/// `std::fs::canonicalize` / Tauri's `resource_dir()` / `current_exe()` can
/// all return `\\?\D:\...` verbatim paths on Windows. The bundled `node.exe`
/// cannot handle a `\\?\`-prefixed *script* path — its `realpathSync` chokes
/// and degrades to `D:`, raising `EISDIR`. Dropping the prefix yields a plain
/// `D:\...` path that node resolves fine. (`\\?\UNC\...` → `\\...`.)
fn strip_verbatim(p: &std::path::Path) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("\\\\?\\") {
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{}", unc));
        }
        return std::path::PathBuf::from(rest.to_string());
    }
    p.to_path_buf()
}

/// Collect candidate base directories that may contain `runtime-host`.
fn candidate_bases(resource_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(dir) = &resource_dir {
        bases.push(dir.clone());
        if let Ok(c) = dir.canonicalize() {
            bases.push(c);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            bases.push(p.to_path_buf());
        }
    }
    bases.into_iter().map(|p| strip_verbatim(&p)).collect()
}

/// Locate the bundled `runtime-host` directory (used by the updater).
fn runtime_host_dir(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    for base in candidate_bases(resource_dir) {
        let dir = base.join("runtime-host");
        if dir.join("package.json").exists() {
            return Some(dir);
        }
    }
    None
}

/// Version information for the locally installed and latest published dsh.
pub struct UpdateInfo {
    pub current: Option<String>,
    pub latest: String,
    pub outdated: bool,
}

const NPM_VIEW_TIMEOUT_SECS: u64 = 30;
const NPM_INSTALL_TIMEOUT_SECS: u64 = 300;

/// Read the installed `@deepseek-ai/dsh` version from `node_modules`.
fn current_version(dir: &std::path::Path) -> Option<String> {
    let pkg = dir
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("package.json");
    let text = std::fs::read_to_string(pkg).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str().map(ToOwned::to_owned)
}

/// Return the version of the bundled/offline `@deepseek-ai/dsh` CLI used by this
/// project (from `runtime-host/node_modules/@deepseek-ai/dsh/package.json`).
/// Returns `None` when the offline package is not present (we fall back to
/// `npx` at runtime).
pub fn current_dsh_version(resource_dir: Option<PathBuf>) -> Option<String> {
    for base in candidate_bases(resource_dir) {
        let dir = base.join("runtime-host");
        if let Some(v) = current_version(&dir) {
            return Some(v);
        }
    }
    None
}

/// Run a command with a timeout while capturing stdout/stderr.
fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    dir: &std::path::Path,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, String, String), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let child = cmd
        .spawn()
        .map_err(|e| format!("启动 {program} 失败: {e}"))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    let _handle = thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    let output = match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(format!("等待 {program} 失败: {e}")),
        Err(_) => {
            eprintln!(
                "[dsh] {program} 超时（{}s），终止进程 pid={pid}",
                timeout.as_secs()
            );
            kill_by_pid(pid);
            let detail = match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(output)) => String::from_utf8_lossy(&output.stderr).trim().to_string(),
                Ok(Err(e)) => format!("等待 {program} 失败: {e}"),
                Err(_) => "无法终止进程".to_string(),
            };
            return Err(format!(
                "{program} 执行超时（{}s）{}",
                timeout.as_secs(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Ok((output.status, stdout, stderr))
}

/// Locate the bundled npm CLI entry inside `runtime-host`.
///
/// npm is installed as a standalone package under `runtime-host/tools/npm`
/// (`npm install npm@<ver> --prefix tools/npm`), so it is *outside* the
/// `@deepseek-ai/dsh` dependency graph — running `npm install` never touches
/// the tool directory. (Bundling pnpm inside the same `node_modules` instead
/// proved fragile: pnpm would rebuild/relink itself during `pnpm add`,
/// corrupting its own entry and leaving the package at the old version.)
/// Falls back to the flat `node_modules/npm/bin/npm-cli.js` layout for older
/// bundles.
fn find_npm_cli(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let candidates: Vec<std::path::PathBuf> = vec![
        dir.join("tools")
            .join("npm")
            .join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
        dir.join("node_modules")
            .join("npm")
            .join("bin")
            .join("npm-cli.js"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Run the bundled `npm` (located inside `runtime-host`) using the bundled
/// Node binary. This makes the updater self-contained on every platform — it
/// does NOT rely on any Node/npm installed on the end user's machine.
fn run_npm(
    dir: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(std::process::ExitStatus, String, String), String> {
    let node = dir.join(node_bin_name());
    let npm = find_npm_cli(dir).ok_or_else(|| {
        "runtime-host 内未找到自带的 npm（tools/npm/node_modules/npm/bin/npm-cli.js）。\
         请重新生成离线包后重试：cd runtime-host && npm install && npm install npm@11 --prefix tools/npm"
            .to_string()
    })?;
    let node_s = node.to_string_lossy().to_string();
    let npm_s = npm.to_string_lossy().to_string();
    let mut all: Vec<String> = vec![npm_s];
    for a in args {
        all.push(a.to_string());
    }
    let refs: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
    run_command_with_timeout(&node_s, &refs, dir, timeout)
}

/// Query the latest published version from the registry.
fn latest_version(dir: &std::path::Path, timeout: Duration) -> Result<String, String> {
    let (status, stdout, stderr) =
        run_npm(dir, &["view", "@deepseek-ai/dsh", "version", "--silent"], timeout)
            .map_err(|e| format!("npm view 失败: {e}"))?;
    if !status.success() {
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("查询最新版本失败: {detail}"));
    }
    let version = stdout.lines().next().unwrap_or("").trim().to_string();
    if version.is_empty() {
        Err("查询最新版本未返回版本号".to_string())
    } else {
        Ok(version)
    }
}

/// Install the latest `@deepseek-ai/dsh` package into `runtime-host`.
///
/// npm keeps the flat `node_modules/@deepseek-ai/dsh/lib/bin.js` layout that
/// `spawn_dsh_web` expects and updates `package.json` / `package-lock.json`
/// (which are npm-native), so the install never restructures `node_modules`
/// the way a pnpm layout migration would.
fn install_latest(dir: &std::path::Path, timeout: Duration) -> Result<(), String> {
    let (status, stdout, stderr) = run_npm(
        dir,
        &[
            "install",
            "@deepseek-ai/dsh@latest",
            "--save-exact",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ],
        timeout,
    )
    .map_err(|e| format!("npm install 失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        Err(format!("更新安装失败: {detail}"))
    }
}

/// Check the installed version against the latest published version.
pub fn check_update(resource_dir: Option<PathBuf>) -> Result<UpdateInfo, String> {
    let dir =
        runtime_host_dir(resource_dir).ok_or_else(|| "未找到 runtime-host 目录".to_string())?;
    eprintln!("[dsh] checking update in {}", dir.display());
    let current = current_version(&dir);
    let latest = latest_version(&dir, Duration::from_secs(NPM_VIEW_TIMEOUT_SECS))?;
    let outdated = current.as_deref() != Some(latest.as_str());
    Ok(UpdateInfo {
        current,
        latest,
        outdated,
    })
}

/// Install the latest `@deepseek-ai/dsh` package into `runtime-host`.
pub fn install_update(resource_dir: Option<PathBuf>) -> Result<(), String> {
    let dir =
        runtime_host_dir(resource_dir).ok_or_else(|| "未找到 runtime-host 目录".to_string())?;
    eprintln!("[dsh] installing package in {}", dir.display());
    install_latest(&dir, Duration::from_secs(NPM_INSTALL_TIMEOUT_SECS))
}

/// Convenience wrapper: check, install, and return a user-facing summary.
#[allow(dead_code)]
pub fn update_dsh(resource_dir: Option<PathBuf>) -> Result<String, String> {
    let info = check_update(resource_dir.clone())?;
    if !info.outdated {
        return Ok(format!("已是最新 v{}", info.latest));
    }
    install_update(resource_dir)?;
    Ok(format!(
        "v{} → v{} 更新完成",
        info.current.as_deref().unwrap_or("?"),
        info.latest
    ))
}

/// Spawn `dsh web` and return `(url, pid)` once the readiness line appears.
pub fn launch_and_wait(resource_dir: Option<PathBuf>) -> Option<(String, u32)> {
    let mut child = spawn_dsh_web(resource_dir)?;
    let pid = child.id();
    eprintln!("[dsh] spawned (pid={pid}), waiting for readiness line ...");

    // Read both stdout and stderr concurrently. `dsh web` may print its ready
    // URL to either stream, so scanning only stdout (the old behaviour) could
    // miss it and leave the app stuck on the loading screen forever.
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let (tx, rx) = mpsc::channel::<String>();
    spawn_line_reader(stdout, tx.clone());
    spawn_line_reader(stderr, tx);

    // Watchdog: if we never become ready, force-kill the process tree.
    let ready = Arc::new(AtomicBool::new(false));
    let ready_w = ready.clone();
    let pid_w = pid;
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(READY_TIMEOUT_SECS));
        if !ready_w.load(Ordering::SeqCst) {
            eprintln!("[dsh] timeout after {READY_TIMEOUT_SECS}s");
            kill_by_pid(pid_w);
        }
    });

    let start = Instant::now();
    for line in rx {
        if let Some(url) = parse_readiness_line(&line) {
            ready.store(true, Ordering::SeqCst);
            // Detach the child so the server keeps running until the app
            // explicitly kills it by pid on exit.
            std::mem::forget(child);
            eprintln!(
                "[dsh] ready in {:.1}s -> {url}",
                start.elapsed().as_secs_f64()
            );
            return Some((url, pid));
        }
    }
    None
}

/// Pump a reader's lines into a channel. The channel closes (and `rx`'s
/// iterator ends) once both reader threads drop their senders — i.e. when the
/// child process exits and both pipes are closed.
fn spawn_line_reader<R: std::io::Read + Send + 'static>(reader: R, tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

/// Which launch path `spawn_dsh_web` will take: the bundled offline CLI
/// (`"offline"`) or the `npx` fallback (`"npx"`). Used to show the right
/// loading message (the npx download can take a while on first run).
pub fn launch_mode(resource_dir: Option<PathBuf>) -> &'static str {
    for base in candidate_bases(resource_dir) {
        let node = base.join("runtime-host").join(node_bin_name());
        let cli = base.join("runtime-host/node_modules/@deepseek-ai/dsh/lib/bin.js");
        if node.exists() && cli.exists() {
            return "offline";
        }
    }
    "npx"
}

/// Parse the readiness line and return a clean loopback origin URL.
///
/// Rather than matching a fixed `dsh web: ` prefix, we scan for a loopback
/// origin (`http://127.0.0.1:<port>` or `http://localhost:<port>`) anywhere in
/// the line. `dsh web` may emit slightly different banner formats (e.g.
/// vite-style `Local: http://127.0.0.1:5173`), and tolerating them avoids a
/// silent "stuck on loading screen".
fn parse_readiness_line(line: &str) -> Option<String> {
    let l = line.trim_end_matches(['\r', '\n']);
    for host in ["http://127.0.0.1:", "http://localhost:"] {
        if let Some(pos) = l.find(host) {
            let after = &l[pos + host.len()..];
            let port: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !port.is_empty() {
                // Normalise `localhost` -> `127.0.0.1` so the navigation URL is
                // always a clear loopback address.
                return Some(format!("http://127.0.0.1:{port}"));
            }
        }
    }
    None
}

/// Spawn the harness. Prefers the bundled offline CLI, falls back to `npx`.
///
/// `resource_dir()` can be reported in a few different shapes across Tauri
/// builds (a canonical path, a drive-relative prefix like `D:`, or the exe
/// directory). To be robust we try each candidate base dir in turn and pick
/// the first one that actually contains the bundled `runtime-host/`.
fn spawn_dsh_web(resource_dir: Option<PathBuf>) -> Option<Child> {
    for base in candidate_bases(resource_dir) {
        let node = base.join("runtime-host").join(node_bin_name());
        let cli = base.join("runtime-host/node_modules/@deepseek-ai/dsh/lib/bin.js");
        eprintln!(
            "[dsh] checking base={}  node={}  cli={}",
            base.display(),
            node.display(),
            cli.display()
        );
        if node.exists() && cli.exists() {
            eprintln!("[dsh] using bundled offline CLI (base={})", base.display());
            let mut cmd = Command::new(&node);
            cmd.arg(&cli)
                .args(["web", "--host", "127.0.0.1", "--port", "0"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                // Pipe stderr too: `dsh web` may print its readiness URL there,
                // and we scan both streams so detection never silently fails.
                // `inherit` would hand node a NULL handle (GUI app, no console)
                // and, without CREATE_NO_WINDOW, pop a console window.
                .stderr(Stdio::piped());
            #[cfg(windows)]
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            return cmd.spawn().ok();
        }
    }

    // Fallback: npx (downloads the package on first run).
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", "npx", "-y", "@deepseek-ai/dsh", "web"])
            .args(["--host", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
            .ok()
    }

    #[cfg(not(windows))]
    {
        Command::new("npx")
            .args(["-y", "@deepseek-ai/dsh", "web"])
            .args(["--host", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()
    }
}

/// Kill a previously-spawned `dsh web` process tree by PID.
#[cfg(windows)]
pub fn kill_by_pid(pid: u32) {
    eprintln!("[dsh] stopping server (pid={pid}) ...");
    let _ = Command::new("cmd")
        .args(["/C", "taskkill", "/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x08000000)
        .status();
}

#[cfg(not(windows))]
pub fn kill_by_pid(pid: u32) {
    eprintln!("[dsh] stopping server (pid={pid}) ...");
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}
