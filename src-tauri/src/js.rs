//! Compile-time embedded injected JavaScript.
//!
//! The actual scripts live in `src-tauri/js/*.js` so they get proper syntax
//! highlighting/linting in editors instead of hiding inside Rust string
//! literals. `include_str!` inlines them into the binary — there is still no
//! runtime file dependency and no frontend toolchain.

/// Injected after every navigation: custom title bar + menus + dialogs.
pub const TITLEBAR_INJECT_JS: &str = include_str!("../js/titlebar.js");

/// Injected when falling back to `npx` so the loading page explains the wait.
pub const LOADING_NPX_JS: &str = include_str!("../js/loading-npx.js");

/// Loading-error page with a runtime-supplied reason.
///
/// The reason is JSON-encoded (via serde_json) so quotes/backslashes/newlines
/// cannot break out of the string literal in the template.
pub fn loading_error_js(reason: &str) -> String {
    const TEMPLATE: &str = include_str!("../js/loading-error.js");
    let encoded = serde_json::to_string(reason).unwrap_or_else(|_| "\"\"".to_string());
    TEMPLATE.replace("__REASON__", &encoded)
}
