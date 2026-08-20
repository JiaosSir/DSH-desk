//! 托盘图标与菜单：显示/隐藏、重启宿主、打开日志目录、退出。
//! 托盘常驻模式留 v1.5（规格 §7 注释），本阶段菜单动作与窗口共存。

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tauri_plugin_opener::OpenerExt;

use crate::ShellCommand;

pub fn init(app: &mut tauri::App) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "重启宿主", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "logs", "打开日志目录", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &restart, &logs, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("应用图标").clone())
        .tooltip("DSH-desk")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_window(app),
            "restart" => restart_host(app),
            "logs" => open_logs(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn restart_host(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let _ = state.cmd_tx.send(ShellCommand::Restart);
}

fn open_logs(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let dir = state.logs_dir.clone();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = app.opener().reveal_item_in_dir(dir);
    }
}
