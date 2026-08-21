//! DSH-desk：DeepSeek Harness Web UI 的 Tauri 2 壳。
//!
//! 壳自身不含前端：`apps/desktop/dist/` 只承载本地等待页/错误页。启动时壳
//! 监督随包分发的 harness sidecar（自带 node + 钉版 `@deepseek-ai/dsh`），
//! 等 sidecar 打印就绪 URL 行后把 WebView 导航到 `http://127.0.0.1:<port>`；
//! 崩溃按退避自动重启，耗尽后展示错误页。原生能力经 `window.__DSH_DESK__`
//! 桥暴露给 Web UI 里的桥接插件。

mod bridge;
mod commands;
pub mod smoke;
mod shortcuts;
mod tray;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use desk_core::logs::RollingLog;
use desk_core::supervisor::{Supervisor, SupervisorEvent, SupervisorOptions};
use desk_core::{logs, paths, ports, profile};
use tauri::Manager;

use crate::commands::DesktopStatus;

/// 壳 → 监督任务的命令通道。
pub enum ShellCommand {
    /// 重启宿主（托盘「重启宿主」/错误页「重试」）：重置尝试计数，换新端口。
    Restart,
    /// 有意停止（退出前）。
    Stop,
}

/// sidecar 关键路径（setup 解析一次，命令与监督任务共用）。
#[derive(Clone)]
pub struct SidecarPaths {
    pub root: PathBuf,
    pub node_exe: PathBuf,
    pub bin_js: PathBuf,
    pub pnpm_dir: PathBuf,
}

/// 应用级共享状态：状态快照（命令读取）与命令通道（命令写入）。
pub struct AppState {
    pub status: Arc<Mutex<DesktopStatus>>,
    pub cmd_tx: tokio::sync::mpsc::UnboundedSender<ShellCommand>,
    pub logs_dir: PathBuf,
    pub sidecar: SidecarPaths,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 二次启动：聚焦已有窗口（规格 §7 单实例锁）。
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::desktop_state,
            commands::desktop_retry,
            commands::desktop_open_logs,
            commands::desktop_quit,
            commands::desktop_get_onboarding,
            commands::desktop_open_releases,
            commands::desktop_set_autostart,
            commands::desktop_get_autostart,
            commands::desktop_get_hotkey,
            commands::desktop_notify,
            commands::desktop_sync_list,
            commands::desktop_sync_add,
        ])
        .setup(|app| {
            let logs_dir = paths::logs_dir();
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<ShellCommand>();
            let status = Arc::new(Mutex::new(DesktopStatus::starting()));
            // sidecar 路径解析一次：监督任务与同步导入命令共用。
            let sidecar = {
                let root = resolve_sidecar_root(&app.handle());
                let (node_exe, bin_js) = sidecar_paths(&root);
                SidecarPaths {
                    pnpm_dir: root.join("pnpm"),
                    root,
                    node_exe,
                    bin_js,
                }
            };
            app.manage(AppState {
                status: Arc::clone(&status),
                cmd_tx,
                logs_dir: logs_dir.clone(),
                sidecar,
            });
            // 窗口在代码里创建：等待页之外还需注入 __DSH_DESK__ 桥。
            // WebView 硬化：仅允许 tauri:// 本地页与本机 sidecar 源，外链交系统浏览器；
            // release 构建禁 devtools（规格 §6.6）。
            // release 构建禁 devtools（规格 §6.6）；debug 下无需重赋值。
            #[allow(unused_mut)]
            let mut window_builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("DSH-desk")
            .inner_size(1280.0, 800.0)
            .center()
            .initialization_script(bridge::BRIDGE_SCRIPT)
            .on_navigation(|url| {
                if url.scheme() == "tauri" || url.host_str() == Some("127.0.0.1") {
                    return true;
                }
                let _ = open::that(url.as_str());
                false
            });
            #[cfg(not(debug_assertions))]
            {
                window_builder = window_builder.devtools(false);
            }
            let window = window_builder.build()?;
            tray::init(app)?;
            shortcuts::init(app)?;
            // 监督任务：拥有 Supervisor，消费命令通道，镜像事件到状态与 WebView。
            tauri::async_runtime::spawn(supervisor_task(
                app.handle().clone(),
                window,
                status,
                cmd_rx,
            ));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // 规格 §7：关闭窗口默认退出进程（托盘驻留留 v1.5）。
                // 监督任务里 Supervisor/Running 的 Drop 会 kill sidecar。
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_, _| {});
}

/// 监督任务主循环：spawn → 等就绪 → 导航；崩溃/端口冲突换端口重试。
async fn supervisor_task(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    status: Arc<Mutex<DesktopStatus>>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ShellCommand>,
) {
    // sidecar 路径：setup 已解析并存于 AppState。
    let sidecar = app.state::<AppState>().sidecar.clone();
    let node_exe = sidecar.node_exe;
    let bin_js = sidecar.bin_js;
    let pnpm_dir = sidecar.pnpm_dir;
    let (log_tx, mut log_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 日志：sidecar 原始行 + 壳事件各一个滚动文件。
    let date = logs::today_compact();
    let sidecar_log = RollingLog::new(&app.state::<AppState>().logs_dir, "sidecar", &date)
        .unwrap_or_else(|e| panic!("无法创建 sidecar 日志: {e}"));
    let shell_log = RollingLog::new(&app.state::<AppState>().logs_dir, "shell", &date)
        .unwrap_or_else(|e| panic!("无法创建 shell 日志: {e}"));
    let _ = shell_log.append("壳启动");

    // 首启初始化 desktop profile（幂等）：web-app 钉 sidecar 同版本、bridge 缺则补。
    // 失败直接进错误页——宿主版本错配会崩，不能让监督循环空转。
    if let Err(e) = initialize_profile(&sidecar.root, &node_exe, &bin_js, &shell_log).await {
        let _ = shell_log.append(&format!("profile 初始化失败: {e}"));
        set_status(&status, DesktopStatus::failed(e.clone()));
        navigate_error(&window, &e);
        return;
    }

    let mut sup = Supervisor::new(SupervisorOptions::default());
    sup.set_log_sink(log_tx);

    // 日志转发必须是独立任务：sidecar 每行输出都会即时到达，若放进监督
    // select 里，收日志会不断取消 sup.wait()，导致 Running 被 drop、
    // 子进程被杀、监督循环反复重启（实测踩坑）。
    tauri::async_runtime::spawn(async move {
        while let Some(line) = log_rx.recv().await {
            let _ = sidecar_log.append(&line);
        }
    });

    // sidecar 环境：零遥测开关 + 自带 pnpm 注入 PATH（dsh plugin 命令的 .cmd 语义）。
    let mut envs: Vec<(String, String)> =
        vec![("DSH_TELEMETRY_DISABLED".to_owned(), "1".to_owned())];
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    envs.push((
        "PATH".to_owned(),
        format!(
            "{}{}{}",
            pnpm_dir.display(),
            if cfg!(windows) { ";" } else { ":" },
            inherited_path.to_string_lossy()
        ),
    ));

    let mut failed = false;
    let node = node_exe.to_string_lossy().into_owned();
    let bin = bin_js.to_string_lossy().into_owned();
    loop {
        // 需要启动时（首启/崩溃后）选空闲端口拉起。
        if !failed && !sup.is_running() {
            let port = match ports::pick_free_port() {
                Ok(port) => port,
                Err(e) => {
                    failed = true;
                    set_status(&status, DesktopStatus::failed(format!("选端口失败: {e}")));
                    let _ = shell_log.append(&format!("选端口失败: {e}"));
                    navigate_error(&window, &format!("选端口失败: {e}"));
                    continue;
                }
            };
            let port_str = port.to_string();
            let _ = shell_log.append(&format!(
                "启动宿主 node {} --profile desktop --port {port}",
                bin_js.display()
            ));
            if let Err(e) = sup
                .start(
                    port,
                    &node,
                    &[&bin, "--profile", "desktop", "--port", &port_str, "--no-open"],
                    &envs,
                )
                .await
            {
                failed = true;
                set_status(&status, DesktopStatus::failed(e.clone()));
                let _ = shell_log.append(&format!("启动失败: {e}"));
                navigate_error(&window, &e);
                continue;
            }
            set_status(&status, DesktopStatus::waiting(port));
        }

        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                Some(ShellCommand::Restart) => {
                    // 重置尝试计数并换新端口：stop 后循环体自会重新 start。
                    let _ = shell_log.append("收到重启宿主请求");
                    sup.stop();
                    let _ = sup.wait().await; // 回收 Stopped
                    set_status(&status, DesktopStatus::starting());
                }
                Some(ShellCommand::Stop) | None => {
                    let _ = shell_log.append("收到停止请求，退出监督");
                    sup.stop();
                    let _ = sup.wait().await;
                    return;
                }
            },
            evt = sup.wait(), if sup.is_running() => match evt {
                SupervisorEvent::Ready { url } => {
                    let _ = shell_log.append(&format!("宿主就绪: {url}"));
                    set_status(&status, DesktopStatus::running(url.clone()));
                    let _ = window.navigate(tauri::Url::parse(&url).expect("就绪 URL 合法"));
                }
                SupervisorEvent::Exited { code, attempt } => {
                    let _ = shell_log.append(&format!("宿主退出（attempt {attempt}，退出码 {code:?}）"));
                    // 就绪前退出：换端口重试由循环体的 start 完成；就绪后崩溃监督器已自动重启。
                    set_status(&status, DesktopStatus::waiting(sup.current_port().unwrap_or(0)));
                }
                SupervisorEvent::Failed { reason } => {
                    failed = true;
                    let _ = shell_log.append(&format!("宿主启动失败: {reason}"));
                    set_status(&status, DesktopStatus::failed(reason.clone()));
                    navigate_error(&window, &reason);
                }
                SupervisorEvent::Stopped => {
                    let _ = shell_log.append("宿主已停止");
                }
            },
        }
    }
}

/// 首启初始化 desktop profile：读 sidecar 钉版 → ensure_profile_init（幂等）。
async fn initialize_profile(
    sidecar_root: &Path,
    node_exe: &Path,
    bin_js: &Path,
    shell_log: &RollingLog,
) -> Result<(), String> {
    let dsh_version = profile::read_dsh_version(sidecar_root)?;
    let bridge_spec = std::env::var("DSH_DESK_BRIDGE_SPEC")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("@JiaosSir/dsh-desk-bridge@{}", env!("CARGO_PKG_VERSION")));
    let opts = profile::InitOptions {
        profile_dir: paths::profile_dir(),
        profile_name: "desktop".to_owned(),
        node_exe: node_exe.to_owned(),
        bin_js: bin_js.to_owned(),
        pnpm_dir: sidecar_root.join("pnpm"),
        dsh_version,
        bridge_spec,
    };
    let _ = shell_log.append("初始化 desktop profile…");
    let outcome = profile::ensure_profile_init(&opts).await?;
    for add in &outcome.ran_adds {
        let _ = shell_log.append(&format!("profile add: {add}"));
    }
    for warn in &outcome.warnings {
        let _ = shell_log.append(warn);
    }
    Ok(())
}

/// 解析 sidecar 根目录：`SIDECAR_ROOT` 环境变量优先（开发/冒烟），
/// 否则开发构建回退源码目录、生产构建用打包的 `<资源目录>/sidecar-dist`。
fn resolve_sidecar_root(app: &tauri::AppHandle) -> PathBuf {
    if let Some(root) = paths::sidecar_root() {
        return root;
    }
    // 开发构建（cargo run / tauri dev）不打包 bundle.resources，resource_dir 里
    // 可能是过期的残缺副本；直接用源码目录（CARGO_MANIFEST_DIR）下的 sidecar-dist。
    #[cfg(debug_assertions)]
    {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecar-dist");
        if src.join("node").join("node.exe").exists() {
            return src;
        }
    }
    app.path()
        .resource_dir()
        .map(|dir| dir.join("sidecar-dist"))
        .unwrap_or_default()
}

/// 由 sidecar 根目录推导 node.exe 与 dsh bin.js 的绝对路径。
pub(crate) fn sidecar_paths(root: &std::path::Path) -> (PathBuf, PathBuf) {
    let node_exe = root.join("node").join("node.exe");
    let bin_js = root
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    (node_exe, bin_js)
}

fn set_status(status: &Arc<Mutex<DesktopStatus>>, next: DesktopStatus) {
    *status.lock().expect("状态锁") = next;
}

/// 导航到错误页，附失败原因（URL 编码经 query 参数传递）。
fn navigate_error(window: &tauri::WebviewWindow, reason: &str) {
    let url = format!("error.html?reason={}", urlencode(reason));
    let _ = window.navigate(
        tauri::Url::parse(&format!("tauri://localhost/{url}"))
            .unwrap_or_else(|_| tauri::Url::parse("tauri://localhost/error.html").expect("静态")),
    );
}

/// 极简 query 编码（错误原因只进 reason 参数，不引依赖）。
fn urlencode(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_owned(),
            '&' => "%26".to_owned(),
            '#' => "%23".to_owned(),
            '?' => "%3F".to_owned(),
            '%' => "%25".to_owned(),
            c if c.is_ascii_alphanumeric() || "-_.~".contains(c) => c.to_string(),
            c => c
                .to_string()
                .as_bytes()
                .iter()
                .map(|b| format!("%{b:02X}"))
                .collect(),
        })
        .collect()
}
