// DeepSeek Harness Launcher — Tauri 2 desktop shell.
//
// Behaviour (mirrors the upstream desktop app):
//  * On startup spawns `dsh web` (bundled offline CLI if present, else `npx`)
//    on a system-assigned loopback port (`--port 0`).
//  * Reads the readiness line from stdout/stderr and navigates the main
//    webview to it once ready (after a loading screen).
//  * Lives in the system tray: closing the window only hides it; the app
//    quits only via the tray "Quit" menu.
//  * On exit it actively stops the `dsh web` process tree through the owned
//    child handle (see `state`/`dsh` modules).
//
// Module map:
//  * `dsh`       — process spawning/readiness/updater plumbing
//  * `state`     — managed state + service start/stop/restart lifecycle
//  * `commands`  — Tauri IPC commands for the injected title-bar UI
//  * `js`        — compile-time embedded injected JavaScript (src-tauri/js)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dsh;
mod js;
mod state;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};

use state::AppState;

fn main() {
    tauri::Builder::default()
        // Single instance: launching the exe again must surface the existing
        // window (which may be hidden in the tray), not open a second app.
        // The callback runs in the *first* instance when a second one starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            setup_tray(app)?;

            // --- spawn dsh web in a background thread, navigate when ready ---
            // The managed state must be fetched *inside* the thread via the
            // cloned AppHandle: a borrowed `&AppState` is not `'static` and
            // cannot cross the `std::thread::spawn` boundary.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let resource_dir = handle.path().resource_dir().ok();
                // If no bundled offline runtime-host exists we fall back to
                // `npx`, whose first-run download can take a while — tell the
                // user so the loading page does not look frozen.
                if dsh::launch_mode(resource_dir.clone()) == "npx" {
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.eval(js::LOADING_NPX_JS);
                    }
                }
                if let Err(e) = state::start_service(&handle) {
                    eprintln!("[dsh] failed to launch harness: {e}");
                    // Surface a clear error instead of leaving the app
                    // stuck on the loading spinner forever.
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.eval(js::loading_error_js(&e).as_str());
                    }
                }
            });

            Ok(())
        })
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::window_action,
            commands::check_dsh_update,
            commands::update_dsh,
            commands::restart_dsh,
            commands::get_whale_icon_url,
            commands::get_about_info
        ])
        .on_window_event(|window, event| {
            // Closing the window hides it (tray owns the lifetime), it does not quit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running DeepSeek Harness launcher")
        .run(|app, event| {
            // Actively stop the dsh web server when the app exits.
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                state::stop_titlebar_injection(app);
                state::stop_server(&app.state::<AppState>());
            }
        });
}

/// Build the system-tray icon + menu.
///
/// Uses a DPI-matched whale PNG instead of the full-size app icon: Windows
/// renders the tray at 16/20/24/28/32px, and downscaling the 256px default
/// window icon there looks blurry. Pick the exact native size for the monitor
/// the main window is on.
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let scale = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let tray_icon = if scale >= 2.0 {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-32.png"))
    } else if scale >= 1.75 {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-28.png"))
    } else if scale >= 1.5 {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-24.png"))
    } else if scale >= 1.25 {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-20.png"))
    } else {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-16.png"))
    }
    .expect("tray icon png");

    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show_item, &PredefinedMenuItem::separator(app)?, &quit_item],
    )?;

    TrayIconBuilder::new()
        .icon(tray_icon)
        .tooltip("DeepSeek Harness")
        .menu(&menu)
        // Left click pops the window straight up (no menu on left click); the
        // menu stays on right click.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
