//! dsh-desktop: the Tauri 2 shell for DeepSeek Harness Web UI.
//!
//! The shell hosts no frontend of its own: `apps/desktop/dist/` only carries
//! the local waiting/error pages. On startup the shell supervises the bundled
//! harness sidecar (node + pinned `@deepseek-ai/dsh`) and navigates the WebView
//! to `http://127.0.0.1:<port>` once the sidecar prints its ready URL line.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application")
}
