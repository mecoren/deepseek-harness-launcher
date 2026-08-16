fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "window_action",
            "check_dsh_update",
            "update_dsh",
            "get_whale_icon_url",
        ]),
    ))
    .expect("failed to run tauri-build");
}
