//! Tauri IPC commands exposed to the injected title-bar UI.
//!
//! NOTE for maintainers: every `#[tauri::command]` here must also be listed in
//! BOTH `build.rs` (`AppManifest::commands`) AND
//! `capabilities/default.json` (`allow-*` permissions), otherwise the ACL
//! rejects calls from the remote loopback origin at runtime.

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::dsh;
use crate::state;
use crate::state::AppState;

/// Brand whale icon (same bytes as `icons/icon.png`), embedded at compile time
/// and served to the title-bar JS as a data URL by [`get_whale_icon_url`].
pub const WHALE_ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

#[derive(Serialize)]
#[allow(dead_code)]
pub struct UpdateCheckResult {
    current: Option<String>,
    latest: String,
    outdated: bool,
    message: String,
}

/// Progress events emitted on `dsh_update_progress` while `update_dsh` runs,
/// so the injected UI can drive its progress bar by phase.
#[derive(Clone, Serialize)]
struct UpdateProgress {
    phase: String,
    percent: u8,
    message: String,
}

/// Custom title-bar window controls, callable from the injected script via
/// `__TAURI_INTERNALS__.invoke('window_action', { action })`. Returns the new
/// maximized state for toggle/query so the maximize glyph stays in sync.
#[tauri::command]
pub fn window_action(app: AppHandle, action: String) -> Result<Option<bool>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    match action.as_str() {
        "minimize" => {
            window.minimize().map_err(|e| e.to_string())?;
            Ok(None)
        }
        "toggle_maximize" => {
            if window.is_maximized().map_err(|e| e.to_string())? {
                window.unmaximize().map_err(|e| e.to_string())?;
            } else {
                window.maximize().map_err(|e| e.to_string())?;
            }
            let maximized = window.is_maximized().map_err(|e| e.to_string())?;
            Ok(Some(maximized))
        }
        "query_maximized" => {
            let maximized = window.is_maximized().map_err(|e| e.to_string())?;
            Ok(Some(maximized))
        }
        "close" => {
            window.close().map_err(|e| e.to_string())?;
            Ok(None)
        }
        other => Err(format!("unknown action: {other}")),
    }
}

/// Check the latest published `@deepseek-ai/dsh` version against the local one.
#[tauri::command]
pub async fn check_dsh_update(app: AppHandle) -> Result<UpdateCheckResult, String> {
    let resource_dir = app.path().resource_dir().ok();
    tauri::async_runtime::spawn_blocking(move || {
        let info = dsh::check_update(resource_dir)?;
        let current = info.current.clone();
        let latest = info.latest.clone();
        let message = if info.outdated {
            format!("v{} → v{}", current.as_deref().unwrap_or("?"), latest)
        } else {
            format!("已是最新 v{}", latest)
        };
        Ok(UpdateCheckResult {
            current: info.current,
            latest: info.latest,
            outdated: info.outdated,
            message,
        })
    })
    .await
    .map_err(|e| format!("检查更新任务失败: {e}"))?
}

/// Update the local `@deepseek-ai/dsh` package used by this project.
///
/// Stops the running `dsh web` first (avoid EBUSY on Windows), installs the
/// new package, and reports progress over the `dsh_update_progress` event.
///
/// Failure recovery: the service was already stopped when npm runs, so an
/// install failure would leave the user with a dead service. We therefore try
/// to relaunch the previous installation before reporting the error. The
/// relaunch navigates the webview back to a working page — the progress
/// dialog dies with the old page, which is acceptable and expected here (the
/// alternative is a permanently broken app behind a dialog).
#[tauri::command]
pub async fn update_dsh(app: tauri::AppHandle) -> Result<String, String> {
    let resource_dir = app.path().resource_dir().ok();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |phase: &str, percent: u8, message: &str| {
            let _ = app2.emit(
                "dsh_update_progress",
                UpdateProgress {
                    phase: phase.to_string(),
                    percent,
                    message: message.to_string(),
                },
            );
        };

        let info = dsh::check_update(resource_dir.clone())?;
        if !info.outdated {
            return Ok(format!("已是最新 v{}", info.latest));
        }

        let old = info.current.clone().unwrap_or_else(|| "?".to_string());
        let new = info.latest.clone();

        // Stop existing dsh web before touching node_modules (avoid EBUSY).
        emit("stopping", 15, "正在停止 DeepSeek Harness 服务…");
        {
            let state = app2.state::<AppState>();
            state::stop_server(&state);
        }

        // Install the new package. The install itself is a single blocking step;
        // the UI switches to an indeterminate bar for this phase.
        emit("installing", 40, &format!("正在下载并安装 v{new} …"));
        match dsh::install_update(resource_dir) {
            Ok(()) => {
                emit("done", 100, "安装完成，请重启服务生效");
                Ok(format!("v{old} → v{new} 更新完成"))
            }
            Err(e) => {
                // 尽力把旧版本拉起来，别让应用停在“服务已停止”的黑洞里。
                emit("recovering", 30, "安装失败，正在恢复服务…");
                match state::restart_service(&app2) {
                    Ok(()) => emit("error", 0, &format!("更新失败：{e}。已恢复原服务。")),
                    Err(re) => emit("error", 0, &format!("更新失败：{e}；服务恢复也失败：{re}")),
                }
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| format!("更新任务失败: {e}"))?
}

/// Restart the local `dsh web` server. Called from the UI after an update
/// (user picked "restart now") or from the menu item as a manual recovery.
#[tauri::command]
pub async fn restart_dsh(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || state::restart_service(&app))
        .await
        .map_err(|e| format!("重启任务失败: {e}"))?
}

/// Return the DeepSeek whale brand icon as a `data:image/png;base64,...` URL so
/// the injected title-bar script can render it as an `<img>` (with CSS filter
/// for dark-mode inversion).
#[tauri::command]
pub fn get_whale_icon_url() -> Result<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(WHALE_ICON_PNG);
    Ok(format!("data:image/png;base64,{b64}"))
}

/// Information shown in the About dialog: the launcher's own version (from
/// `Cargo.toml`) and the version of the `@deepseek-ai/dsh` CLI bundled for this
/// project (offline runtime-host). `dsh_version` is `None` when the offline
/// package is absent and we fall back to `npx` at runtime.
#[derive(Serialize)]
pub struct AboutInfo {
    launcher_version: String,
    dsh_version: Option<String>,
}

#[tauri::command]
pub fn get_about_info(app: AppHandle) -> Result<AboutInfo, String> {
    let resource_dir = app.path().resource_dir().ok();
    Ok(AboutInfo {
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        dsh_version: dsh::current_dsh_version(resource_dir),
    })
}
