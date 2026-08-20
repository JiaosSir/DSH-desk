//! 壳侧 IPC 命令：桥接插件（window.__DSH_DESK__）调用的后端实现。
//! 阶段 2 实现首批命令；阶段 3/4 追加文件夹选择、自启、通知、Releases 等。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
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

/// 首次引导状态：工作区（`~/.dsh/desktop/config.json`）+ 是否已有 API key。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOnboarding {
    pub workspace: Option<String>,
    pub has_api_key: bool,
}

/// 首次引导状态：工作区读桌面配置；hasApiKey 只判键名存在性（不读值）。
#[tauri::command]
pub fn desktop_get_onboarding() -> DesktopOnboarding {
    let config = desk_core::config::load();
    DesktopOnboarding {
        workspace: config.workspace,
        has_api_key: desk_core::credentials::has_api_key(&desk_core::paths::dsh_home()),
    }
}

/// 原生文件夹选择 → 写 config.json 的 workspace → 返回所选路径（取消返回 null）。
#[tauri::command]
pub async fn desktop_pick_workspace(app: AppHandle) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(path.and_then(|p| p.into_path().ok()));
    });
    let chosen = rx.await.ok().flatten()?;
    let mut config = desk_core::config::load();
    config.workspace = Some(chosen.to_string_lossy().into_owned());
    let _ = desk_core::config::save(&config);
    Some(chosen.to_string_lossy().into_owned())
}
