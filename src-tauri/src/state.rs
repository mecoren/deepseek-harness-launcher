//! Shared application state and `dsh web` service lifecycle orchestration.
//!
//! Lifecycle rules:
//!  * At most one server instance lives in [`AppState::server`] at any time.
//!  * Every start goes through [`start_service`], every stop through
//!    [`stop_server`] — `restart` is exactly stop-then-start, so restarting
//!    can no longer leak the previous process tree.
//!  * The title-bar injection loop is a single background thread whose
//!    generation handle ([`AppState::inject_flag`]) flips whenever a new
//!    navigation starts a fresh loop, so stale loops always exit promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::dsh;
use crate::js;

/// Managed Tauri state.
pub struct AppState {
    /// The running `dsh web` server, if any. Holding the child handle pins the
    /// OS process object: the pid cannot be recycled while we store it.
    pub server: Mutex<Option<dsh::DshServer>>,
    /// Stop-flag of the currently running title-bar injection thread.
    pub inject_flag: Mutex<Option<Arc<AtomicBool>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            server: Mutex::new(None),
            inject_flag: Mutex::new(None),
        }
    }
}

/// Stop the running `dsh web` server (if any). Returns `true` when a live
/// server was stopped.
pub fn stop_server(state: &AppState) -> bool {
    let mut guard = state.server.lock().unwrap();
    match guard.take() {
        Some(mut srv) => {
            srv.kill_tree();
            true
        }
        None => false,
    }
}

/// Launch `dsh web`, navigate the main window to it and (re)start the
/// title-bar injection loop. Fails with a user-displayable reason.
pub fn start_service(app: &AppHandle) -> Result<(), String> {
    let resource_dir = app.path().resource_dir().ok();
    let server = dsh::launch_and_wait(resource_dir)?;

    let url = server.url().to_string();
    {
        let state = app.state::<AppState>();
        // Defensive: never accumulate servers. launch_and_wait owns a fresh
        // child; if an old one somehow still exists, retire it first.
        stop_server(&state);
        *state.server.lock().unwrap() = Some(server);
    }

    let parsed = url::Url::parse(&url).map_err(|e| format!("无效地址 {url}: {e}"))?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .navigate(parsed)
        .map_err(|e| format!("导航失败: {e}"))?;

    start_titlebar_injection(app);
    Ok(())
}

/// Stop the current server, then start a fresh one. This is THE restart path
/// used by the update flow and the manual menu item — the old process tree is
/// always reaped before a new one spawns.
pub fn restart_service(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        stop_server(&state);
    }
    start_service(app)
}

/// (Re)arm the title-bar injection loop.
///
/// Any previously running injection thread observes its stop-flag within
/// ~50 ms and exits, so repeated navigations/restarts never pile up threads
/// hammering `eval()` against the same page.
fn start_titlebar_injection(app: &AppHandle) {
    let flag = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut guard = state.inject_flag.lock().unwrap();
        if let Some(old) = guard.replace(flag.clone()) {
            old.store(true, Ordering::SeqCst);
        }
    }
    let window = match app.get_webview_window("main") {
        Some(w) => w,
        None => return,
    };
    let js = js::TITLEBAR_INJECT_JS.to_string();
    thread::spawn(move || {
        // Keep injecting across SPA refreshes; the script itself is idempotent.
        // Sleep in small slices so a stop request is honoured quickly instead
        // of up to one full period later.
        loop {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            let _ = window.eval(js.as_str());
            for _ in 0..10 {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    });
}

/// Stop the injection loop (used on app exit).
pub fn stop_titlebar_injection(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Bind the guard explicitly: a temporary guard inside an `if let`
    // scrutinee would be dropped after `state` under edition-2021 rules.
    let mut guard = state.inject_flag.lock().unwrap();
    if let Some(old) = guard.take() {
        old.store(true, Ordering::SeqCst);
    }
}
