//! 冒烟自测（env `DSH_DESK_SMOKE=1` 的无窗口路径，计划 D9 / 任务 3.3）：
//! 初始化 profile → 拉起 sidecar → 等就绪 → GET / 断言 200 与标题 → 停 sidecar。
//!
//! 成功打印 `SMOKE_OK` 并 exit 0；任何失败打印 `SMOKE_FAILED: <原因>` 并 exit 1。

use std::path::PathBuf;

use desk_core::supervisor::{Supervisor, SupervisorEvent, SupervisorOptions};
use desk_core::{paths, ports, profile};

/// 冒烟入口（以 0/1 退出进程，不返回）。
pub fn run() -> ! {
    let runtime = tokio::runtime::Runtime::new().expect("冒烟 tokio runtime 初始化失败");
    match runtime.block_on(smoke()) {
        Ok(()) => {
            println!("SMOKE_OK");
            std::process::exit(0);
        }
        Err(reason) => {
            println!("SMOKE_FAILED: {reason}");
            std::process::exit(1);
        }
    }
}

async fn smoke() -> Result<(), String> {
    let sidecar_root = resolve_sidecar_root();
    let (node_exe, bin_js) = crate::sidecar_paths(&sidecar_root);

    // 初始化 desktop profile（与壳同逻辑：web-app 钉版，bridge 失败不阻塞）。
    let dsh_version = profile::read_dsh_version(&sidecar_root)?;
    let bridge_spec = std::env::var("DSH_DESK_BRIDGE_SPEC")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("@cjiaojiao/dsh-desk-bridge@{}", env!("CARGO_PKG_VERSION")));
    let outcome = profile::ensure_profile_init(&profile::InitOptions {
        profile_dir: paths::profile_dir(),
        profile_name: "desktop".to_owned(),
        node_exe: node_exe.clone(),
        bin_js: bin_js.clone(),
        pnpm_dir: sidecar_root.join("pnpm"),
        dsh_version,
        bridge_spec,
    })
    .await?;
    for warn in outcome.warnings {
        eprintln!("冒烟警告: {warn}");
    }

    // 拉起 sidecar 等就绪。
    let port = ports::pick_free_port().map_err(|e| format!("选端口失败: {e}"))?;
    let port_str = port.to_string();
    let node = node_exe.to_string_lossy().into_owned();
    let bin = bin_js.to_string_lossy().into_owned();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let envs: Vec<(String, String)> = vec![
        ("DSH_TELEMETRY_DISABLED".to_owned(), "1".to_owned()),
        (
            "PATH".to_owned(),
            format!(
                "{}{}{}",
                sidecar_root.join("pnpm").display(),
                if cfg!(windows) { ";" } else { ":" },
                inherited.to_string_lossy()
            ),
        ),
    ];
    let mut sup = Supervisor::new(SupervisorOptions::default());
    sup.start(
        port,
        &node,
        &[&bin, "--profile", "desktop", "--port", &port_str, "--no-open"],
        &envs,
    )
    .await?;
    let url = match sup.wait().await {
        SupervisorEvent::Ready { url } => url,
        SupervisorEvent::Exited { code, .. } => {
            return Err(format!("sidecar 提前退出（退出码 {code:?}）"))
        }
        SupervisorEvent::Failed { reason } => return Err(reason),
        SupervisorEvent::Stopped => return Err("sidecar 意外停止".to_owned()),
    };

    // 健康检查：GET / 断言 200 与标题（apps/web/index.html 的真实标题）。
    let response = ureq::get(&url).call().map_err(|e| format!("GET / 失败: {e}"))?;
    if response.status() != 200 {
        return Err(format!("GET / 状态码 {}", response.status()));
    }
    let body = response.into_string().map_err(|e| format!("读响应失败: {e}"))?;
    if !body.contains("<title>DeepSeek Harness</title>") {
        return Err("页面标题不含 DeepSeek Harness".to_owned());
    }

    sup.stop();
    let _ = sup.wait().await;
    Ok(())
}

/// 冒烟 sidecar 根目录：`SIDECAR_ROOT` 环境优先；否则源码目录（构建机/CI），
/// 再否则 exe 旁（安装包布局）。返回值统一去过 verbatim（\\?\）前缀。
fn resolve_sidecar_root() -> PathBuf {
    let root = if let Some(root) = paths::sidecar_root() {
        root
    } else {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecar-dist");
        if src.join("node").join("node.exe").exists() {
            src
        } else {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.join("sidecar-dist")))
                .unwrap_or_default()
        }
    };
    paths::deverbatim(&root)
}
