//! 壳侧 IPC 命令：桥接插件（`window.__DSH_DESK__`）调用的后端实现。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::update::{self, UpdateInfo, UpdateMode, UpdateProgress};
use crate::{AppState, ShellCommand};

/// 壳状态快照（等待页/错误页经 desktop_state 读取）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    /// 生命周期阶段：preparing / starting / waiting / running / failed。
    pub phase: &'static str,
    /// 就绪后的 Web UI 地址。
    pub url: Option<String>,
    /// 当前端口（未启动为 None）。
    pub port: Option<u16>,
    /// 失败原因（failed 阶段）。
    pub error: Option<String>,
    /// 准备阶段进度（0.0..=1.0；仅 preparing 阶段有值，等待页据此画进度条）。
    pub progress: Option<f64>,
}

impl DesktopStatus {
    pub fn starting() -> Self {
        Self {
            phase: "starting",
            url: None,
            port: None,
            error: None,
            progress: None,
        }
    }

    /// sidecar 缓存解压（方案 A 首启）：`progress` 为 0.0..=1.0。
    pub fn preparing(progress: f64) -> Self {
        Self {
            phase: "preparing",
            url: None,
            port: None,
            error: None,
            progress: Some(progress),
        }
    }

    pub fn waiting(port: u16) -> Self {
        Self {
            phase: "waiting",
            url: None,
            port: Some(port),
            error: None,
            progress: None,
        }
    }

    pub fn running(url: String) -> Self {
        Self {
            phase: "running",
            url: Some(url),
            port: None,
            error: None,
            progress: None,
        }
    }

    pub fn failed(error: String) -> Self {
        Self {
            phase: "failed",
            url: None,
            port: None,
            error: Some(error),
            progress: None,
        }
    }
}

/// 页面类别：本地资产页（等待页/错误页）与 sidecar 页面。桥接脚本每次加载
/// 上报，壳据此决定由页面自行 `location.replace` 导航（历史干净）还是
/// `webview.navigate` push 兜底。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    /// 本地资产页（tauri:// 等待页/错误页）。
    Asset,
    /// sidecar Web UI（http://127.0.0.1:*）。
    Host,
}

/// 页面类别上报（桥接脚本每次页面加载调用，fire-and-forget）。
#[tauri::command]
pub fn desktop_page_kind(state: State<'_, AppState>, kind: PageKind) {
    *state.page_kind.lock().expect("页面状态锁") = Some(kind);
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

/// 退出桌面应用：停 sidecar 后退出进程（与关窗/托盘退出同路径，带确认）。
#[tauri::command]
pub fn desktop_quit(app: AppHandle) -> Result<(), String> {
    crate::stop_host_then_exit(&app);
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
        .open_url(
            "https://github.com/JiaosSir/DSH-desk/releases",
            None::<&str>,
        )
        .map_err(|e| format!("打开 Releases 失败: {e}"))
}

/// 检查更新：查询 GitHub Releases 最新版本并与当前版本比较；把待下载资产与
/// 更新模式记入会话（便携版仅提示，不做应用内更新）。
#[tauri::command]
pub async fn desktop_check_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateInfo, String> {
    let current = update::effective_current_version(&app.package_info().version.to_string());
    let mode = update::detect_mode();
    let info =
        tauri::async_runtime::spawn_blocking(move || update::check_for_update(&current, mode))
            .await
            .map_err(|e| format!("检查更新任务异常: {e}"))??;
    let mut session = state.update.lock().expect("更新会话锁");
    session.record_check(mode, &info);
    Ok(info)
}

/// 下载新版安装包（仅安装版；便携版在 check 结果里已提示手动下载）。
/// 进度经 `desktop_update_progress` 轮询。
#[tauri::command]
pub async fn desktop_download_update(state: State<'_, AppState>) -> Result<(), String> {
    let (pending, session) = {
        let s = state.update.lock().expect("更新会话锁");
        if s.mode != UpdateMode::Installed {
            return Err(
                "便携版不支持应用内更新，请前往 GitHub Releases 下载最新压缩包后手动覆盖"
                    .to_owned(),
            );
        }
        let pending = s.pending.clone().ok_or_else(|| "请先检查更新".to_owned())?;
        (pending, std::sync::Arc::clone(&state.update))
    };
    let dest = std::env::temp_dir().join(&pending.name);
    tauri::async_runtime::spawn_blocking(move || {
        update::download_update(&session, &pending, &dest)
    })
    .await
    .map_err(|e| format!("下载更新任务异常: {e}"))??;
    Ok(())
}

/// 当前更新进度（UI 轮询）。
#[tauri::command]
pub fn desktop_update_progress(state: State<'_, AppState>) -> UpdateProgress {
    state.update.lock().expect("更新会话锁").progress.clone()
}

/// 安装已下载的新版本（仅安装版）：先经临时脚本等本应用退出，再静默覆盖安装
/// 并自动重启；应用随即优雅退出（停 sidecar，避免孤儿进程）。
#[tauri::command]
pub fn desktop_install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let (path, mode) = {
        let s = state.update.lock().expect("更新会话锁");
        (s.downloaded.clone(), s.mode)
    };
    if mode != UpdateMode::Installed {
        return Err(
            "便携版不支持应用内更新，请前往 GitHub Releases 下载最新压缩包后手动覆盖".to_owned(),
        );
    }
    let path = path.ok_or_else(|| "安装包尚未下载完成".to_owned())?;
    if !path.exists() {
        return Err("安装包不存在，请重新下载".to_owned());
    }
    {
        let mut s = state.update.lock().expect("更新会话锁");
        s.progress.phase = "installing";
    }
    // 顺序不能反：先优雅停 sidecar（安装器静默模式会自行结束本进程，此时宿主
    // 已停、不会留孤儿 sidecar），再直接拉起安装器（GUI 子进程，无控制台窗口），
    // 最后退出；装完 /R 自动拉起新版本。
    crate::stop_host(&app);
    update::launch_installer(&path)?;
    app.exit(0);
    Ok(())
}

/// 设置开机自启；返回持久化后的状态。
#[tauri::command]
pub fn desktop_set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| format!("启用自启失败: {e}"))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("禁用自启失败: {e}"))?;
    }
    manager
        .is_enabled()
        .map_err(|e| format!("读取自启状态失败: {e}"))
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

/// 当前标题栏模式（config.json：native / hidden）。
#[tauri::command]
pub fn desktop_get_titlebar_mode() -> desk_core::config::TitlebarMode {
    desk_core::config::load().titlebar
}

/// 切换标题栏模式：持久化到 config.json，立即切换窗口装饰，并通知
/// WebView 重建/移除自绘透明标题栏（桥接脚本监听 desktop_titlebar 事件）。
#[tauri::command]
pub fn desktop_set_titlebar_mode(
    app: tauri::AppHandle,
    mode: desk_core::config::TitlebarMode,
) -> Result<desk_core::config::TitlebarMode, String> {
    let mut cfg = desk_core::config::load();
    cfg.titlebar = mode;
    desk_core::config::save(&cfg)?;
    let decorated = mode == desk_core::config::TitlebarMode::Native;
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_decorations(decorated)
            .map_err(|e| format!("切换窗口装饰失败: {e}"))?;
        // 事件名与桥接脚本里的监听一致；WebView 收到后重建/移除透明条。
        let _ = window.emit("desktop_titlebar", mode);
    }
    Ok(mode)
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

/// web→desktop 同步差集（web 有而 DSHdesk 无的 `name@version`）。
#[tauri::command]
pub fn desktop_sync_list() -> Vec<String> {
    let home = desk_core::paths::dsh_home();
    desk_core::profile::compute_sync_diff(
        &home.join("profiles").join("web"),
        &home.join("profiles").join(desk_core::profile::PROFILE_NAME),
    )
    .missing
}

/// 从 web 导入一个插件到 desktop profile（壳跑 `dsh plugin add`）。
#[tauri::command]
pub async fn desktop_sync_add(pkg: String, state: State<'_, AppState>) -> Result<(), String> {
    let sidecar = state.sidecar.clone();
    // release 首启时缓存可能尚未解压：防御性校验，sidecar 未就绪时给出明确错误而非 node 启动失败。
    if !sidecar.node_exe.exists() {
        return Err("宿主环境未就绪（sidecar 尚未就绪，请稍后重试）".to_owned());
    }
    desk_core::profile::add_profile_plugin(
        &desk_core::profile::PluginAddOptions {
            profile_dir: desk_core::paths::profile_dir(),
            profile_name: desk_core::profile::PROFILE_NAME.to_owned(),
            node_exe: sidecar.node_exe,
            bin_js: sidecar.bin_js,
            pnpm_dir: sidecar.pnpm_dir,
        },
        &pkg,
    )
    .await
}
