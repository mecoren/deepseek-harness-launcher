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

/// Prefix of the canonical readiness line printed by `dsh web`.
const READINESS_PREFIX: &str = "dsh web: ";
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

/// Run the bundled `pnpm` (located inside `runtime-host`) using the bundled
/// Node binary. This makes the updater self-contained on every platform — it
/// does NOT rely on any Node/npm installed on the end user's machine.
///
/// `runtime-host/.npmrc` sets `node-linker=hoisted` so pnpm keeps the flat
/// `node_modules/@deepseek-ai/dsh/lib/bin.js` layout that `spawn_dsh_web`
/// expects (instead of pnpm's default symlinked layout, which breaks once the
/// bundle is copied/installed on another machine).
fn run_pnpm(
    dir: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(std::process::ExitStatus, String, String), String> {
    let node = dir.join(node_bin_name());
    let pnpm = dir
        .join("node_modules")
        .join("pnpm")
        .join("bin")
        .join("pnpm.cjs");
    let node_s = node.to_string_lossy().to_string();
    let pnpm_s = pnpm.to_string_lossy().to_string();
    let mut all: Vec<String> = vec![pnpm_s];
    for a in args {
        all.push(a.to_string());
    }
    let refs: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
    run_command_with_timeout(&node_s, &refs, dir, timeout)
}

/// Query the latest published version from the registry.
fn latest_version(dir: &std::path::Path, timeout: Duration) -> Result<String, String> {
    let (status, stdout, stderr) = run_pnpm(dir, &["info", "@deepseek-ai/dsh", "version"], timeout)
        .map_err(|e| format!("pnpm info 失败: {e}"))?;
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
fn install_latest(dir: &std::path::Path, timeout: Duration) -> Result<(), String> {
    let (status, stdout, stderr) =
        run_pnpm(dir, &["add", "@deepseek-ai/dsh", "--save-exact"], timeout)
            .map_err(|e| format!("pnpm add 失败: {e}"))?;
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

    let stdout = child.stdout.take()?;
    let reader = BufReader::new(stdout).lines();

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
    for line in reader {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
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

/// Parse the readiness line and return a clean loopback origin URL.
fn parse_readiness_line(line: &str) -> Option<String> {
    let l = line.trim_end_matches(['\r', '\n']);
    if !l.starts_with(READINESS_PREFIX) {
        return None;
    }
    let rest = &l[READINESS_PREFIX.len()..];
    let token = rest.split(|c: char| c.is_whitespace()).next()?;
    if token.is_empty() {
        return None;
    }
    let (scheme_host, after) = token.rsplit_once(':')?;
    if !(scheme_host == "http://127.0.0.1" || scheme_host == "http://localhost") {
        return None;
    }
    let port: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if port.is_empty() {
        return None;
    }
    Some(format!("{scheme_host}:{port}"))
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
                // stderr -> NUL: the launcher is a GUI app without a console;
                // `inherit` would hand node a NULL handle (writes fail) and,
                // without CREATE_NO_WINDOW, Windows pops a new console window
                // for the console-subsystem node.exe — closing it kills dsh web
                // and leaves the app stuck on the loading screen.
                .stderr(Stdio::null());
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
            .stderr(Stdio::inherit())
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
            .stderr(Stdio::inherit())
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
