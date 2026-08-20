//! Desktop profile initialization and the web→desktop plugin sync diff.
//!
//! [`ensure_profile_init`] 是幂等的首启初始化：profile 缺 web-app 时用钉死的
//! harness 版本 add，缺 bridge 时用 spec add。两次 add 都经 sidecar 自带
//! node + dsh bin.js 执行（`--profile` 传 profile **名字**，不是路径），env 注入
//! 自带 pnpm 到 PATH 头部。web-app 失败是致命错误（宿主起不来）；bridge 失败
//! 只记 warning（桥接插件未发布/未链接时不阻塞启动，宿主照常可用）。
//!
//! 关键不变量：profile 里 web-app 的版本必须等于 sidecar 的 dsh 版本
//! （否则宿主 loader 会因客户端插件版本错配而崩溃——见 2026-08-20 的
//! `Unknown file extension ".css"` 事故），因此 web-app 的判定是**版本感知**的：
//! 缺失或版本不符都会重新钉版。
//!
//! [`compute_sync_diff`] 计算 web profile 有而 desktop profile 无的插件差集，
//! 供设置区「从 web 导入」用。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// desktop profile 必装的两层：web-app（钉 harness 版本）与桥接插件。
pub const WEB_APP_PACKAGE: &str = "@deepseek-ai/dsh-web-app";
pub const BRIDGE_PACKAGE: &str = "@JiaosSir/dsh-desk-bridge";

/// 初始化参数：所有外部路径与版本由壳注入，便于离线单测（桩 node + 桩 bin.js）。
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// desktop profile 目录（`$DSH_HOME/profiles/desktop`），用于幂等判定。
    pub profile_dir: PathBuf,
    /// desktop profile 名（`dsh plugin --profile <name>` 用名字，不是路径）。
    pub profile_name: String,
    /// sidecar 自带 node 可执行文件（node.exe）。
    pub node_exe: PathBuf,
    /// sidecar 自带 dsh CLI 入口（`node_modules/@deepseek-ai/dsh/lib/bin.js`）。
    pub bin_js: PathBuf,
    /// sidecar 自带 pnpm 目录（注入子进程 PATH 头部）。
    pub pnpm_dir: PathBuf,
    /// 钉死的 harness 版本（与 sidecar 一致），用于 pin web-app。
    pub dsh_version: String,
    /// bridge 安装 spec（env `DSH_DESK_BRIDGE_SPEC` 优先，缺省 `@JiaosSir/dsh-desk-bridge@<壳版本>`）。
    pub bridge_spec: String,
}

/// 初始化结果：按顺序记录本次实际执行的 `add <pkg>` 与非致命警告。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileInitOutcome {
    pub ran_adds: Vec<String>,
    /// 非致命失败（如 bridge 未发布），已记录但未阻塞启动。
    pub warnings: Vec<String>,
}

/// web→desktop 同步差集：web 有而 desktop 无的插件（`name@version`）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncDiff {
    pub missing: Vec<String>,
}

/// 幂等初始化 desktop profile。缺 web-app（或版本不符）→ 钉版 add；缺 bridge → add spec。
pub async fn ensure_profile_init(opts: &InitOptions) -> Result<ProfileInitOutcome, String> {
    let deps = read_dependencies(&opts.profile_dir)?;
    let mut ran_adds = Vec::new();
    let mut warnings = Vec::new();

    // web-app 版本感知：只有缺失或版本不符才重新钉版（防止 profile 与 sidecar 错配）。
    let web_app_spec = format!("{WEB_APP_PACKAGE}@{}", opts.dsh_version);
    if deps.get(WEB_APP_PACKAGE).map(String::as_str) != Some(opts.dsh_version.as_str()) {
        run_add_robust(opts, &web_app_spec).await?;
        ran_adds.push(web_app_spec);
    }
    // bridge 只判存在性（版本随壳同号，缺了才补）。失败不阻塞：桥接插件是
    // 可选增强（未发布/未链接时宿主照常可用），只记 warning。
    if !deps.contains_key(BRIDGE_PACKAGE) {
        match run_add_robust(opts, &opts.bridge_spec).await {
            Ok(()) => ran_adds.push(opts.bridge_spec.clone()),
            Err(e) => warnings.push(format!("bridge add 失败（不阻塞启动）: {e}")),
        }
    }
    Ok(ProfileInitOutcome { ran_adds, warnings })
}

/// web profile 有而 desktop profile 无的插件差集（按 web 的 `name@version` 列出）。
/// 任一 profile 缺失或不可读都按空处理（不报错）。
pub fn compute_sync_diff(web_dir: &Path, desktop_dir: &Path) -> SyncDiff {
    let web = read_dependencies(web_dir).unwrap_or_default();
    let desktop = read_dependencies(desktop_dir).unwrap_or_default();
    let missing = web
        .into_iter()
        .filter(|(name, _)| !desktop.contains_key(name))
        .map(|(name, version)| format!("{name}@{version}"))
        .collect();
    SyncDiff { missing }
}

/// 读取 `<dir>/package.json` 的 `dependencies`（`name → version`）。
/// 文件缺失视为空依赖；JSON 非法则报错（fail loud）。
fn read_dependencies(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let manifest = dir.join("package.json");
    let text = match std::fs::read_to_string(&manifest) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(format!("读取 {} 失败: {e}", manifest.display())),
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("解析 {} 失败: {e}", manifest.display()))?;
    let mut deps = BTreeMap::new();
    if let Some(obj) = value.get("dependencies").and_then(|d| d.as_object()) {
        for (name, version) in obj {
            if let Some(v) = version.as_str() {
                deps.insert(name.clone(), v.to_string());
            }
        }
    }
    Ok(deps)
}

/// 经 sidecar 自带 node 执行 `dsh plugin --profile <name> add <pkg>`；
/// env 注入自带 pnpm 到 PATH 头部 + 零遥测开关。非零退出返回带尾部输出的错误。
async fn run_add(opts: &InitOptions, pkg: &str) -> Result<(), String> {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let path = format!(
        "{}{sep}{}",
        opts.pnpm_dir.display(),
        inherited.to_string_lossy()
    );

    let output = tokio::process::Command::new(&opts.node_exe)
        .arg(&opts.bin_js)
        .args(["plugin", "--profile", &opts.profile_name])
        .args(["add", pkg])
        .env("PATH", path)
        .env("DSH_TELEMETRY_DISABLED", "1")
        .output()
        .await
        .map_err(|e| format!("dsh plugin add 启动失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(2000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(format!(
            "dsh plugin add {pkg} 失败（退出码 {:?}）: {tail}",
            output.status.code()
        ));
    }
    Ok(())
}

/// 执行 add：失败时修复 allowBuilds 占位符并重试一次。全新 profile 的第一次
/// add 必然因 harness 模板的占位符（`koffi: set this to true or false`）失败；
/// 修复后重试即成功。
async fn run_add_robust(opts: &InitOptions, pkg: &str) -> Result<(), String> {
    let result = run_add(opts, pkg).await;
    if result.is_ok() {
        return result;
    }
    ensure_allow_builds(&opts.profile_dir)?;
    run_add(opts, pkg).await
}

/// 修复 harness profile 模板里的 allowBuilds 占位符：把
/// `koffi: set this to true or false` 换成 `koffi: true`，并补 `node-pty: true`
/// （pnpm ≥10 会因无效布尔拒绝安装）。文件缺失视为无事（尚未初始化）。
fn ensure_allow_builds(profile_dir: &Path) -> Result<(), String> {
    let path = profile_dir.join("pnpm-workspace.yaml");
    let mut text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("读取 {} 失败: {e}", path.display())),
    };
    let mut changed = false;
    if text.contains("koffi: set this to true or false") {
        text = text.replace("koffi: set this to true or false", "koffi: true");
        changed = true;
    }
    if !text.contains("node-pty:") {
        if let Some(pos) = text.find("allowBuilds:") {
            let insert_at = text[pos..]
                .find('\n')
                .map(|i| pos + i + 1)
                .unwrap_or(text.len());
            text.insert_str(insert_at, "  node-pty: true\n");
        } else {
            text.push_str("\nallowBuilds:\n  node-pty: true\n");
        }
        changed = true;
    }
    if changed {
        std::fs::write(&path, text).map_err(|e| format!("写入 {} 失败: {e}", path.display()))?;
    }
    Ok(())
}

/// 读 sidecar 里钉死的 harness 版本（`<sidecar_root>/node_modules/@deepseek-ai/dsh/package.json`）。
pub fn read_dsh_version(sidecar_root: &Path) -> Result<String, String> {
    let manifest = sidecar_root
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("package.json");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("读取 {} 失败: {e}", manifest.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("解析 {} 失败: {e}", manifest.display()))?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("{} 缺少 version 字段", manifest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// 桩 bin.js：解析 argv 里的 `--profile <name>` 与 `add <pkg>`，把
    /// `name pkg` 追加到自身旁的 adds.log；pkg 含 `FAILME` 时退出 1；
    /// 含 `FAILONCE` 时首次退出 1、之后成功。不触网、不真跑 pnpm。
    fn stub_node_script() -> &'static str {
        r#"
const fs = require('node:fs');
const path = require('node:path');
const i = process.argv.indexOf('--profile');
const profile = process.argv[i + 1];
const j = process.argv.indexOf('add');
const pkg = process.argv[j + 1];
fs.appendFileSync(path.join(path.dirname(__filename), 'adds.log'), profile + ' ' + pkg + '\n');
if (pkg && pkg.includes('FAILME')) process.exit(1);
if (pkg && pkg.includes('FAILONCE')) {
  const flag = path.join(path.dirname(__filename), 'failonce.flag');
  if (!fs.existsSync(flag)) {
    fs.writeFileSync(flag, '1');
    process.exit(1);
  }
}
"#
    }

    /// 测试环境是否可用 node（桩 bin.js 的载体）；无 node 时静默跳过。
    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn init_options(profile_dir: &Path, stub: &Path) -> InitOptions {
        InitOptions {
            profile_dir: profile_dir.to_owned(),
            profile_name: "desktop".into(),
            node_exe: "node".into(),
            bin_js: stub.to_owned(),
            pnpm_dir: profile_dir.join("pnpm-stub"),
            dsh_version: "0.1.0-rc.8".into(),
            bridge_spec: "@JiaosSir/dsh-desk-bridge@0.1.0".into(),
        }
    }

    fn write_stub(tmp: &TempDir) -> std::path::PathBuf {
        let stub = tmp.path().join("stub.cjs");
        fs::write(&stub, stub_node_script()).unwrap();
        stub
    }

    #[test]
    fn 读取_sidecar_dsh_版本() {
        let tmp = TempDir::new().unwrap();
        let pkg = tmp
            .path()
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("package.json");
        std::fs::create_dir_all(pkg.parent().unwrap()).unwrap();
        std::fs::write(&pkg, r#"{"version":"0.1.0-rc.8"}"#).unwrap();
        assert_eq!(read_dsh_version(tmp.path()).unwrap(), "0.1.0-rc.8");
    }

    #[test]
    fn 修复_allow_builds_占位符并补_node_pty() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("pnpm-workspace.yaml"),
            "packages:\n  - .\nallowBuilds:\n  koffi: set this to true or false\n",
        )
        .unwrap();
        ensure_allow_builds(tmp.path()).unwrap();
        let text = fs::read_to_string(tmp.path().join("pnpm-workspace.yaml")).unwrap();
        assert!(!text.contains("set this to true or false"));
        assert!(text.contains("koffi: true"));
        assert!(text.contains("node-pty: true"));
    }

    #[tokio::test]
    async fn web_app_add_失败后修复占位符并重试() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        let stub = write_stub(&tmp);
        let mut opts = init_options(&profile, &stub);
        opts.dsh_version = "FAILONCE".into();
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert_eq!(
            outcome.ran_adds,
            vec![
                "@deepseek-ai/dsh-web-app@FAILONCE".to_string(),
                "@JiaosSir/dsh-desk-bridge@0.1.0".to_string(),
            ]
        );
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert_eq!(
            log.matches("@deepseek-ai/dsh-web-app@FAILONCE").count(),
            2,
            "第一次失败后应重试一次"
        );
    }

    #[test]
    fn 空_web_profile_同步差集为空() {
        let tmp = TempDir::new().unwrap();
        let diff = compute_sync_diff(&tmp.path().join("web"), &tmp.path().join("desktop"));
        assert!(diff.missing.is_empty());
    }

    #[test]
    fn 同步差集返回_desktop_缺失的包() {
        let tmp = TempDir::new().unwrap();
        let web = tmp.path().join("web");
        let desktop = tmp.path().join("desktop");
        fs::create_dir_all(&web).unwrap();
        fs::create_dir_all(&desktop).unwrap();
        fs::write(
            web.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.8","@linxin666/dsh-ssh":"0.1.20"}}"#,
        )
        .unwrap();
        fs::write(
            desktop.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.8"}}"#,
        )
        .unwrap();
        let diff = compute_sync_diff(&web, &desktop);
        assert_eq!(diff.missing, vec!["@linxin666/dsh-ssh@0.1.20".to_string()]);
    }

    #[tokio::test]
    async fn 空_profile_依次_add_web_app_与_bridge() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert_eq!(
            outcome.ran_adds,
            vec![
                "@deepseek-ai/dsh-web-app@0.1.0-rc.8".to_string(),
                "@JiaosSir/dsh-desk-bridge@0.1.0".to_string(),
            ]
        );
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        let lines: Vec<&str> = log.trim_end().split('\n').collect();
        assert_eq!(
            lines,
            vec![
                "desktop @deepseek-ai/dsh-web-app@0.1.0-rc.8",
                "desktop @JiaosSir/dsh-desk-bridge@0.1.0",
            ]
        );
    }

    #[tokio::test]
    async fn 幂等_第二次无_add() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.8","@JiaosSir/dsh-desk-bridge":"0.1.0"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.ran_adds.is_empty());
        assert!(outcome.warnings.is_empty());
        assert!(!tmp.path().join("adds.log").exists());
    }

    #[tokio::test]
    async fn bridge_已在则只补_web_app() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@JiaosSir/dsh-desk-bridge":"0.1.0"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert_eq!(
            outcome.ran_adds,
            vec!["@deepseek-ai/dsh-web-app@0.1.0-rc.8".to_string()]
        );
        assert!(outcome.warnings.is_empty());
    }

    #[tokio::test]
    async fn bridge_add_失败_不阻塞初始化() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.8"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let mut opts = init_options(&profile, &stub);
        opts.bridge_spec = "@JiaosSir/dsh-desk-bridge@FAILME".into();
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.ran_adds.is_empty());
        assert_eq!(outcome.warnings.len(), 1);
        assert!(outcome.warnings[0].contains("FAILME"));
    }

    #[tokio::test]
    async fn web_app_版本不符时重新钉版() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.7"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert_eq!(
            outcome.ran_adds,
            vec![
                "@deepseek-ai/dsh-web-app@0.1.0-rc.8".to_string(),
                "@JiaosSir/dsh-desk-bridge@0.1.0".to_string(),
            ]
        );
    }
}
