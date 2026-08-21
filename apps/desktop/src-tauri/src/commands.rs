//! 壳侧 IPC 命令：桥接插件（window.__DSH_DESK__）调用的后端实现。
//! 阶段 2 实现首批命令；阶段 3/4 追加文件夹选择、自启、通知、Releases 等。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, ShellCommand};

/// 壳状态快照（等待页/错误页经 desktop_state 读取）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    /// 生命周期阶段：starting / waiting / running / failed。
    pub phase: &'static str,
    /// 就绪后的 Web UI 地址。
    pub url: Option<String>,
    /// 当前端口（未启动为 None）。
    pub port: Option<u16>,
    /// 失败原因（failed 阶段）。
    pub error: Option<String>,
}

impl DesktopStatus {
    pub fn starting() -> Self {
        Self {
            phase: "starting",
            url: None,
            port: None,
            error: None,
        }
    }

    pub fn waiting(port: u16) -> Self {
        Self {
            phase: "waiting",
            url: None,
            port: Some(port),
            error: None,
        }
    }

    pub fn running(url: String) -> Self {
        Self {
            phase: "running",
            url: Some(url),
            port: None,
            error: None,
        }
    }

    pub fn failed(error: String) -> Self {
        Self {
            phase: "failed",
            url: None,
            port: None,
            error: Some(error),
        }
    }
}

/// 当前壳状态（等待页/错误页轮询显示）。
#[tauri::command]
pub fn desktop_state(state: State<'_, AppState>) -> DesktopStatus {
    state.status.lock().expect("状态锁").clone()
}

/// 重启宿主：先停掉当前 sidecar，监督循环会用新端口重新拉起。
#[tauri::command]
pub fn desktop_retry(state: State<'_, AppState>) -> Result<(), String> {
    state
        .cmd_tx
        .send(ShellCommand::Restart)
        .map_err(|e| format!("命令通道不可用: {e}"))
}

/// 在资源管理器中打开日志目录。
#[tauri::command]
pub fn desktop_open_logs(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.logs_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建日志目录: {e}"))?;
    app.opener()
        .reveal_item_in_dir(dir)
        .map_err(|e| format!("打开日志目录失败: {e}"))
}

/// 退出桌面应用：停 sidecar 后退出进程。
#[tauri::command]
pub fn desktop_quit(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = state.cmd_tx.send(ShellCommand::Stop);
    app.exit(0);
    Ok(())
}

/// 首次引导状态：是否已有 API key（值不读，只判存在性）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOnboarding {
    pub has_api_key: bool,
}

/// 首次引导状态。
#[tauri::command]
pub fn desktop_get_onboarding() -> DesktopOnboarding {
    DesktopOnboarding {
        has_api_key: desk_core::credentials::has_api_key(&desk_core::paths::dsh_home()),
    }
}

/// 打开 GitHub Releases（系统浏览器；用户主动触发，规格 §9）。
#[tauri::command]
pub fn desktop_open_releases(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url("https://github.com/JiaosSir/DSH-desk/releases", None::<&str>)
        .map_err(|e| format!("打开 Releases 失败: {e}"))
}

/// 设置开机自启；返回持久化后的状态。
#[tauri::command]
pub fn desktop_set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| format!("启用自启失败: {e}"))?;
    } else {
        manager.disable().map_err(|e| format!("禁用自启失败: {e}"))?;
    }
    manager.is_enabled().map_err(|e| format!("读取自启状态失败: {e}"))
}

/// 当前开机自启状态。
#[tauri::command]
pub fn desktop_get_autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// 当前全局快捷键（config.json，只读展示）。
#[tauri::command]
pub fn desktop_get_hotkey() -> String {
    desk_core::config::load().hotkey
}

/// 系统通知镜像（审批事件等）。
#[tauri::command]
pub fn desktop_notify(app: AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("系统通知失败: {e}"))
}

/// web→desktop 同步差集（web 有而 desktop 无的 `name@version`）。
#[tauri::command]
pub fn desktop_sync_list() -> Vec<String> {
    let home = desk_core::paths::dsh_home();
    desk_core::profile::compute_sync_diff(
        &home.join("profiles").join("web"),
        &home.join("profiles").join("desktop"),
    )
    .missing
}

/// 从 web 导入一个插件到 desktop profile（壳跑 `dsh plugin add`）。
#[tauri::command]
pub async fn desktop_sync_add(pkg: String, state: State<'_, AppState>) -> Result<(), String> {
    let sidecar = state.sidecar.clone();
    desk_core::profile::add_profile_plugin(
        &desk_core::profile::PluginAddOptions {
            profile_dir: desk_core::paths::profile_dir(),
            profile_name: "desktop".to_owned(),
            node_exe: sidecar.node_exe,
            bin_js: sidecar.bin_js,
            pnpm_dir: sidecar.pnpm_dir,
        },
        &pkg,
    )
    .await
}
