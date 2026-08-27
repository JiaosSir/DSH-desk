//! 全局快捷键：唤起/隐藏主窗口。缺省 `Ctrl+Alt+D`，config.json 的 `hotkey`
//! 可改（v1 只读展示）；非法值回退缺省。

use std::str::FromStr;

use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// 注册全局快捷键（规格 §7：默认 Ctrl+Alt+D 唤起/隐藏）。
pub fn init(app: &mut tauri::App) -> tauri::Result<()> {
    let hotkey = desk_core::config::load().hotkey;
    let shortcut = Shortcut::from_str(&hotkey)
        .unwrap_or_else(|_| Shortcut::from_str("Ctrl+Alt+D").expect("缺省快捷键必须合法"));
    let handle = app.handle().clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_window(&handle);
            }
        })
        .map_err(|e| std::io::Error::other(format!("注册全局快捷键失败: {e}")))?;
    Ok(())
}

fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}
