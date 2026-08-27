//! Desktop profile 初始化与 web→desktop 插件同步差集。
//!
//! [`ensure_profile_init`] 幂等首启：web-app 只登记 bundles 并钉 sidecar 同版本
//! （版本错配会让宿主崩溃），bridge 缺则 add（失败只 warning 不阻塞启动）；
//! [`compute_sync_diff`] 供设置区「从 web 导入」用。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// desktop profile 必装的两层：web-app（钉 harness 版本）与桥接插件。
pub const WEB_APP_PACKAGE: &str = "@deepseek-ai/dsh-web-app";
pub const BRIDGE_PACKAGE: &str = "@cjiaojiao/dsh-desk-bridge";

/// desktop profile 的安装层（与 web profile 模板同构）：只登记进
/// `dsh.profile.bundles`、绝不 `pnpm add`——add 会把 web-app 的依赖全家桶装进
/// profile 的 node_modules，宿主核心插件双副本加载、Symbol 分裂、工具调用崩溃。
pub const REQUIRED_BUNDLES: [&str; 2] = ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"];

/// 废弃包名（改名前的 bridge）：残留的 bundle 层会让加载器撞上
/// `duplicate loader entry id`，初始化时从 bundles 与 dependencies 两处清理。
pub const OBSOLETE_PACKAGES: [&str; 1] = ["@JiaosSir/dsh-desk-bridge"];

/// desktop profile 的名字（`dsh plugin --profile <name>` / `$DSH_HOME/profiles/<name>`）。
/// 带品牌区分的名字，避免与生态里常见的 `desktop` 冲突。
pub const PROFILE_NAME: &str = "DSHdesk";

/// 改名前的旧 profile 名：启动时检测并整体迁移（重命名目录，
/// node_modules/锁文件跟随移动；会话/设置与 profile 名无关，本就共享）。
pub const LEGACY_PROFILE_NAME: &str = "desktop";

/// 默认随装插件（`name@spec`，缺则 add、失败只 warning 不阻塞启动）。
/// 目前为 dsh-market；选型要求：依赖干净（不得含 `@deepseek-ai/*`，防双副本）、
/// 能从 argv 的 `--profile` 解析激活、安装命令经宿主自身的 node + bin.js 重入。
pub const DEFAULT_PLUGINS: [&str; 1] = ["dshmarket@1.31.1"];

/// 默认插件的用户层 patch 配置（`cordis.patch.yml`，幂等）：dsh-market 禁用
/// 自带重启（桌面由壳负责重启，防孤儿进程）并显式钉死安装 profile。
/// 必须用 `concat!` 拼接——续行符会吃掉行首缩进使 YAML 解析失败。
pub const DEFAULT_PLUGIN_PATCH_OVERRIDES: &str = concat!(
    "# DSH-desk 生成的用户层 patch：默认插件的桌面环境配置。\n",
    "# dsh-market：桌面由壳监督宿主生命周期，禁用市场自带重启（防孤儿进程）。\n",
    "- id: dsh-market\n",
    "  config:\n",
    "    allowRestart: false\n",
    "    profile: DSHdesk\n",
);

/// 初始化参数：所有外部路径与版本由壳注入，便于离线单测（桩 node + 桩 bin.js）。
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// desktop profile 目录（`$DSH_HOME/profiles/DSHdesk`），用于幂等判定。
    pub profile_dir: PathBuf,
    /// desktop profile 名（`dsh plugin --profile <name>` 用名字，不是路径）。
    pub profile_name: String,
    /// sidecar 自带 node 可执行文件（node.exe）。
    pub node_exe: PathBuf,
    /// sidecar 自带 dsh CLI 入口（`node_modules/@deepseek-ai/dsh/lib/bin.js`）。
    pub bin_js: PathBuf,
    /// sidecar 自带 pnpm 目录（注入子进程 PATH 头部）。
    pub pnpm_dir: PathBuf,
    /// 钉死的 harness 版本（与 sidecar 一致，保留供诊断/未来校验）。
    pub dsh_version: String,
    /// bridge 安装 spec（env `DSH_DESK_BRIDGE_SPEC` 优先，缺省 `@cjiaojiao/dsh-desk-bridge@<壳版本>`）。
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

/// 幂等初始化 desktop profile：web-app 只登记 bundles（绝不 add，见
/// [`REQUIRED_BUNDLES`]）、历史污染依赖迁移清理、bridge 缺则 add。
pub async fn ensure_profile_init(opts: &InitOptions) -> Result<ProfileInitOutcome, String> {
    let mut ran_adds = Vec::new();
    let mut warnings = Vec::new();

    // 0) 旧 profile 名迁移（desktop → DSHdesk）：重命名目录，node_modules/锁文件跟随移动；失败只 warning。
    match migrate_legacy_profile(&opts.profile_dir) {
        Ok(Some(desc)) => ran_adds.push(format!("profile 迁移：{desc}")),
        Ok(None) => {}
        Err(e) => warnings.push(format!("旧 profile 迁移失败（不阻塞启动）: {e}")),
    }

    // 0.5) 确保 profile 目录存在（全新 DSH_HOME 时缺失，后续 fs::write 不建父目录）。
    //     须在迁移之后：先 rename 旧目录再补建，否则 create_dir_all 会让迁移的短路判定失效。
    std::fs::create_dir_all(&opts.profile_dir)
        .map_err(|e| format!("创建 profile 目录失败 {}: {e}", opts.profile_dir.display()))?;

    // 1) manifest 存在性 + bundles 规范化：安装层前置、去重、废弃名移除。
    if ensure_profile_structure(&opts.profile_dir)? {
        ran_adds.push("bundles 登记（dsh-base + web-app）".to_owned());
    }

    // 2) 历史污染迁移：dependencies 残留 web-app/废弃包名 → 移出并 install 重建
    //    node_modules。失败是致命错误：不清理宿主依旧双副本崩溃，错误页展示原因更诚实。
    let deps = read_dependencies(&opts.profile_dir)?;
    let stale: Vec<&str> = [WEB_APP_PACKAGE]
        .into_iter()
        .chain(OBSOLETE_PACKAGES.iter().copied())
        .filter(|name| deps.contains_key(*name))
        .collect();
    if !stale.is_empty() {
        if !remove_dependencies(&opts.profile_dir, &stale)? {
            return Err("移除 profile 残留依赖失败（manifest 不可写）".to_owned());
        }
        run_install_robust(opts).await?;
        ran_adds.push(format!(
            "依赖迁移：移出 {} 并重建 node_modules",
            stale.join(", ")
        ));
    }

    // 3) bridge 只判存在性（版本随壳同号），缺则 add；失败不阻塞（可选增强，宿主照常可用）。
    let deps = read_dependencies(&opts.profile_dir)?;
    if !deps.contains_key(BRIDGE_PACKAGE) {
        match run_add_robust(opts, &opts.bridge_spec).await {
            Ok(()) => ran_adds.push(opts.bridge_spec.clone()),
            Err(e) => warnings.push(format!("bridge add 失败（不阻塞启动）: {e}")),
        }
    }

    // 4) 默认随装插件（如 dsh-market 插件市场）：缺则 add，失败不阻塞。
    ensure_default_plugins(opts, &DEFAULT_PLUGINS, &mut ran_adds, &mut warnings).await;

    // 5) 默认插件的用户层 patch 配置（幂等）：dsh-market 禁用自带重启 +
    //    显式钉激活 profile。
    if ensure_user_patch_overrides(&opts.profile_dir)? {
        ran_adds.push("用户层 patch：默认插件桌面配置".to_owned());
    }
    Ok(ProfileInitOutcome { ran_adds, warnings })
}

/// 默认随装插件：缺则 add（失败修复 allowBuilds 占位符后重试一次）；装后审查
/// 插件依赖不得含 `@deepseek-ai/*`（防双副本，见 [`REQUIRED_BUNDLES`]），
/// 违规只 warning。失败与违规都不阻塞启动。
async fn ensure_default_plugins(
    opts: &InitOptions,
    plugins: &[&str],
    ran_adds: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    let deps = read_dependencies(&opts.profile_dir).unwrap_or_default();
    for spec in plugins {
        let name = spec.split('@').next().unwrap_or(spec);
        if deps.contains_key(name) {
            continue;
        }
        match run_add_robust(opts, spec).await {
            Ok(()) => {
                ran_adds.push(spec.to_string());
                if let Some(violation) = bundled_dependency_violation(&opts.profile_dir, name) {
                    warnings.push(format!(
                        "默认插件 {name} 的 dependencies 含 {}（可能造成宿主双副本，建议更换插件或手动移除该依赖）: {violation}",
                        "@deepseek-ai/*".to_owned()
                    ));
                }
            }
            Err(e) => warnings.push(format!("默认插件 {name} add 失败（不阻塞启动）: {e}")),
        }
    }
}

/// 依赖审查：返回 `<dir>/node_modules/<name>/package.json` 的 dependencies 中
/// `@deepseek-ai/` 前缀包名（空 = 干净）；包未安装或不可读视为干净。
fn bundled_dependency_violation(dir: &Path, name: &str) -> Option<String> {
    let manifest_path = dir.join("node_modules").join(name).join("package.json");
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let violations: Vec<&str> = value
        .get("dependencies")
        .and_then(|d| d.as_object())
        .map(|deps| {
            deps.keys()
                .filter(|k| k.starts_with("@deepseek-ai/"))
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    if violations.is_empty() {
        None
    } else {
        Some(violations.join(", "))
    }
}

/// 确保 profile 用户层 patch（`cordis.patch.yml`）含默认插件的配置块（幂等）：
/// 已含缩进完整的标准配置则跳过；空数组/损坏/缺配置则写标准块——桌面禁用
/// 市场自带重启是安全要求（防孤儿进程），优先于用户自定义。返回是否写入。
fn ensure_user_patch_overrides(dir: &Path) -> Result<bool, String> {
    let patch_path = dir.join("cordis.patch.yml");
    let marker = "- id: dsh-market";
    let text = match std::fs::read_to_string(&patch_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(&patch_path, DEFAULT_PLUGIN_PATCH_OVERRIDES)
                .map_err(|e| format!("写入 {} 失败: {e}", patch_path.display()))?;
            return Ok(true);
        }
        Err(e) => return Err(format!("读取 {} 失败: {e}", patch_path.display())),
    };
    if text.contains(marker) {
        // 仅当缩进完整的标准配置已存在（`config:` 两空格、allowRestart 四空格）才跳过；
        // 早期生成的缩进损坏文件（`config:` 无缩进）虽含同字样也必须重写（安全要求优先）。
        let correctly_configured =
            text.contains("\n  config:") && text.contains("\n    allowRestart: false");
        if correctly_configured {
            return Ok(false);
        }
        // 损坏/不完整版本：整体替换为标准块。
        std::fs::write(&patch_path, DEFAULT_PLUGIN_PATCH_OVERRIDES)
            .map_err(|e| format!("写入 {} 失败: {e}", patch_path.display()))?;
        return Ok(true);
    }
    // 空数组形态（注释 + `[]`，harness 默认模板）：整体替换为配置块。
    let without_comments: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    if without_comments.trim() == "[]" {
        std::fs::write(&patch_path, DEFAULT_PLUGIN_PATCH_OVERRIDES)
            .map_err(|e| format!("写入 {} 失败: {e}", patch_path.display()))?;
        return Ok(true);
    }
    // 已有其他配置行（数组形态）：末尾追加配置块。
    let mut updated = text;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(DEFAULT_PLUGIN_PATCH_OVERRIDES);
    std::fs::write(&patch_path, updated)
        .map_err(|e| format!("写入 {} 失败: {e}", patch_path.display()))?;
    Ok(true)
}

/// 旧 profile 名迁移：旧名目录存在且新名（`dir`）不存在时整体重命名；
/// 返回迁移描述；失败由调用方记 warning（不阻塞启动）。
fn migrate_legacy_profile(dir: &Path) -> Result<Option<String>, String> {
    let legacy = dir
        .parent()
        .map(|p| p.join(LEGACY_PROFILE_NAME))
        .unwrap_or_default();
    if !legacy.exists() || dir.exists() {
        return Ok(None);
    }
    std::fs::rename(&legacy, dir).map_err(|e| {
        format!(
            "重命名旧 profile 目录 {} → {} 失败: {e}",
            legacy.display(),
            dir.display()
        )
    })?;
    Ok(Some(format!("{LEGACY_PROFILE_NAME} → {PROFILE_NAME}")))
}

/// 确保 profile manifest 存在且 bundles 结构正确（安装层前置、去重、废弃名
/// 移除）。返回是否有写入。
fn ensure_profile_structure(dir: &Path) -> Result<bool, String> {
    let manifest_path = dir.join("package.json");
    let mut value: serde_json::Value = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => serde_json::from_str(&t)
            .map_err(|e| format!("解析 {} 失败: {e}", manifest_path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 全新 profile：模仿 harness initProfile 的最小 manifest（bundles 含安装层，依赖留空）。
            let mut root = serde_json::Map::new();
            root.insert("name".to_owned(), serde_json::json!("dsh-profile-desktop"));
            root.insert("private".to_owned(), serde_json::json!(true));
            root.insert("dependencies".to_owned(), serde_json::json!({}));
            root.insert(
                "dsh".to_owned(),
                serde_json::json!({ "profile": { "bundles": REQUIRED_BUNDLES } }),
            );
            let manifest = serde_json::Value::Object(root);
            write_manifest(&manifest_path, &manifest)?;
            return Ok(true);
        }
        Err(e) => return Err(format!("读取 {} 失败: {e}", manifest_path.display())),
    };
    let current: Vec<String> = value
        .pointer("/dsh/profile/bundles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mut target: Vec<String> = Vec::new();
    for name in REQUIRED_BUNDLES {
        if !target.iter().any(|t| t == name) {
            target.push(name.to_owned());
        }
    }
    for name in &current {
        if REQUIRED_BUNDLES.contains(&name.as_str()) || OBSOLETE_PACKAGES.contains(&name.as_str()) {
            continue;
        }
        if !target.contains(name) {
            target.push(name.clone());
        }
    }
    if current == target {
        return Ok(false);
    }
    let dsh = value
        .as_object_mut()
        .ok_or_else(|| format!("{} 根节点不是对象", manifest_path.display()))?
        .entry("dsh")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{} 的 dsh 字段不是对象", manifest_path.display()))?;
    let profile = dsh
        .entry("profile")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{} 的 dsh.profile 字段不是对象", manifest_path.display()))?;
    profile.insert(
        "bundles".to_owned(),
        serde_json::Value::Array(target.into_iter().map(serde_json::Value::String).collect()),
    );
    write_manifest(&manifest_path, &value)?;
    Ok(true)
}

/// 从 manifest 的 dependencies 移除指定包（保留其他字段）。返回是否实际移除。
fn remove_dependencies(dir: &Path, names: &[&str]) -> Result<bool, String> {
    let manifest_path = dir.join("package.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 {} 失败: {e}", manifest_path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("解析 {} 失败: {e}", manifest_path.display()))?;
    let mut changed = false;
    if let Some(deps) = value
        .pointer_mut("/dependencies")
        .and_then(|d| d.as_object_mut())
    {
        for name in names {
            if deps.remove(*name).is_some() {
                changed = true;
            }
        }
    }
    if changed {
        write_manifest(&manifest_path, &value)?;
    }
    Ok(changed)
}

/// 写回 profile manifest（2 空格缩进 + 换行，与 harness 的 writeProfileManifest 同风格）。
fn write_manifest(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|e| format!("序列化 {} 失败: {e}", path.display()))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|e| format!("写入 {} 失败: {e}", path.display()))
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

/// 给 desktop profile 追加插件的参数（设置区「从 web 导入」用）。
#[derive(Debug, Clone)]
pub struct PluginAddOptions {
    pub profile_dir: PathBuf,
    pub profile_name: String,
    pub node_exe: PathBuf,
    pub bin_js: PathBuf,
    pub pnpm_dir: PathBuf,
}

/// 给 profile 追加一个插件（`dsh plugin --profile <name> add <pkg>`）；
/// 失败时修复 allowBuilds 占位符并重试一次（与 ensure_profile_init 同策略）。
pub async fn add_profile_plugin(opts: &PluginAddOptions, pkg: &str) -> Result<(), String> {
    let result = run_plugin_parts(
        &opts.node_exe,
        &opts.bin_js,
        &opts.pnpm_dir,
        &opts.profile_name,
        &["add", pkg],
    )
    .await;
    if result.is_ok() {
        return result;
    }
    ensure_allow_builds(&opts.profile_dir)?;
    run_plugin_parts(
        &opts.node_exe,
        &opts.bin_js,
        &opts.pnpm_dir,
        &opts.profile_name,
        &["add", pkg],
    )
    .await
}

/// 经 sidecar 自带 node 执行 `dsh plugin --profile <name> add <pkg>`；
/// env 注入自带 pnpm 到 PATH 头部 + 零遥测开关，非零退出返回尾部输出。
async fn run_add(opts: &InitOptions, pkg: &str) -> Result<(), String> {
    run_plugin_parts(
        &opts.node_exe,
        &opts.bin_js,
        &opts.pnpm_dir,
        &opts.profile_name,
        &["add", pkg],
    )
    .await
}

/// 重建 profile 依赖（`dsh plugin --profile <name> install`）——迁移清理
/// node_modules 全家桶用。失败时修复 allowBuilds 占位符并重试一次。
async fn run_install_robust(opts: &InitOptions) -> Result<(), String> {
    let result = run_plugin_parts(
        &opts.node_exe,
        &opts.bin_js,
        &opts.pnpm_dir,
        &opts.profile_name,
        &["install"],
    )
    .await;
    if result.is_ok() {
        return result;
    }
    ensure_allow_builds(&opts.profile_dir)?;
    run_plugin_parts(
        &opts.node_exe,
        &opts.bin_js,
        &opts.pnpm_dir,
        &opts.profile_name,
        &["install"],
    )
    .await
}

/// 经 sidecar 自带 node 执行 `dsh plugin --profile <name> <args...>`（子命令
/// 原样转发给 pnpm）；env 注入自带 pnpm 到 PATH 头部 + 零遥测开关。
async fn run_plugin_parts(
    node_exe: &Path,
    bin_js: &Path,
    pnpm_dir: &Path,
    profile_name: &str,
    args: &[&str],
) -> Result<(), String> {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let path = format!("{}{sep}{}", pnpm_dir.display(), inherited.to_string_lossy());

    let mut cmd = tokio::process::Command::new(node_exe);
    cmd.arg(bin_js)
        .arg("plugin")
        .arg("--profile")
        .arg(profile_name)
        .args(args)
        .env("PATH", path)
        .env("DSH_TELEMETRY_DISABLED", "1");
    // GUI 壳下不弹控制台窗口（同 supervisor 的 CREATE_NO_WINDOW）。
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("dsh plugin 启动失败: {e}"))?;

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
            "dsh plugin {} 失败（退出码 {:?}）: {tail}",
            args.join(" "),
            output.status.code()
        ));
    }
    Ok(())
}

/// 执行 add：失败时修复 allowBuilds 占位符并重试一次——全新 profile 的首次
/// add 必因 harness 模板的占位符（`koffi: set this to true or false`）失败。
async fn run_add_robust(opts: &InitOptions, pkg: &str) -> Result<(), String> {
    let result = run_add(opts, pkg).await;
    if result.is_ok() {
        return result;
    }
    ensure_allow_builds(&opts.profile_dir)?;
    run_add(opts, pkg).await
}

/// 修复 harness profile 模板里的 allowBuilds 占位符（`koffi: set this to true
/// or false` → `true`）并补 `node-pty: true`（pnpm ≥10 会因无效布尔拒绝安装）；
/// 文件缺失视为无事。
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

    /// 桩 bin.js：解析 argv 里的 `--profile` 与子命令（`add <pkg>` / `install`），
    /// 把 `name <pkg|install>` 追加到 adds.log；pkg 含 `FAILME` 退出 1、
    /// 含 `FAILONCE` 首次退出 1 之后成功。不触网、不真跑 pnpm。
    fn stub_node_script() -> &'static str {
        r#"
const fs = require('node:fs');
const path = require('node:path');
const i = process.argv.indexOf('--profile');
const profile = process.argv[i + 1];
const j = process.argv.indexOf('add');
const pkg = j >= 0 ? process.argv[j + 1] : 'install';
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
            profile_name: PROFILE_NAME.to_owned(),
            node_exe: "node".into(),
            bin_js: stub.to_owned(),
            pnpm_dir: profile_dir.join("pnpm-stub"),
            dsh_version: "0.1.0-rc.8".into(),
            bridge_spec: "@cjiaojiao/dsh-desk-bridge@0.1.0".into(),
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

    /// 读取测试 profile 的 manifest 辅助。
    fn manifest_json(dir: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(dir.join("package.json")).unwrap()).unwrap()
    }

    fn bundles_of(dir: &Path) -> Vec<String> {
        manifest_json(dir)["dsh"]["profile"]["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn deps_of(dir: &Path) -> Vec<String> {
        manifest_json(dir)["dependencies"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn bridge_add_失败后修复占位符并重试() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        let stub = write_stub(&tmp);
        let mut opts = init_options(&profile, &stub);
        opts.bridge_spec = "@cjiaojiao/dsh-desk-bridge@FAILONCE".into();
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.warnings.is_empty());
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert_eq!(
            log.matches("@cjiaojiao/dsh-desk-bridge@FAILONCE").count(),
            2,
            "第一次失败后应重试一次"
        );
    }

    #[tokio::test]
    async fn 追加插件_失败后修复占位符并重试() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        let stub = write_stub(&tmp);
        let opts = PluginAddOptions {
            profile_dir: profile.clone(),
            profile_name: PROFILE_NAME.to_owned(),
            node_exe: "node".into(),
            bin_js: stub,
            pnpm_dir: profile.join("pnpm-stub"),
        };
        add_profile_plugin(&opts, "@linxin666/dsh-ssh@FAILONCE")
            .await
            .unwrap();
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert_eq!(
            log.matches("@linxin666/dsh-ssh@FAILONCE").count(),
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
    async fn 空_profile_登记_bundles_并_add_bridge() {
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
                "bundles 登记（dsh-base + web-app）".to_string(),
                "@cjiaojiao/dsh-desk-bridge@0.1.0".to_string(),
                "dshmarket@1.31.1".to_string(),
                "用户层 patch：默认插件桌面配置".to_string(),
            ]
        );
        assert!(outcome.warnings.is_empty());
        // web-app 只登记 bundles，绝不 add（全家桶不进 profile node_modules）。
        assert_eq!(
            bundles_of(&profile),
            vec!["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]
        );
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        let lines: Vec<&str> = log.trim_end().split('\n').collect();
        assert_eq!(
            lines,
            vec![
                "DSHdesk @cjiaojiao/dsh-desk-bridge@0.1.0",
                "DSHdesk dshmarket@1.31.1",
            ]
        );
        // 默认插件的用户层 patch：dsh-market 禁用自带重启 + 钉激活 profile。
        let patch = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(patch.contains("- id: dsh-market"));
        assert!(patch.contains("allowRestart: false"));
        assert!(patch.contains("profile: DSHdesk"));
    }

    #[tokio::test]
    async fn 全新_profile_目录不存在时自动创建() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        // 不预建 profile 目录：模拟全新 DSH_HOME。回归：fs::write 不建父目录，
        // 缺了会 os error 3（CI 冒烟首启事故）。
        let profile = tmp.path().join("profiles").join(PROFILE_NAME);
        assert!(!profile.exists());
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(
            profile.join("package.json").exists(),
            "应自动创建 profile 目录并落 manifest"
        );
        assert!(outcome.warnings.is_empty());
    }

    #[tokio::test]
    async fn 幂等_第二次无操作() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // 理想终态：dependencies 只有 bridge + 默认插件，bundles 含安装层，
        // 用户层 patch 已含默认插件配置。
        fs::write(
            profile.join("package.json"),
            r#"{"name":"dsh-profile-desktop","private":true,"dependencies":{"@cjiaojiao/dsh-desk-bridge":"0.1.0","dshmarket":"1.31.1"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","@cjiaojiao/dsh-desk-bridge","dshmarket"]}}}"#,
        )
        .unwrap();
        fs::write(
            profile.join("cordis.patch.yml"),
            DEFAULT_PLUGIN_PATCH_OVERRIDES,
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
    async fn bundles_缺失时补登记安装层() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@cjiaojiao/dsh-desk-bridge":"0.1.0"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert_eq!(
            outcome.ran_adds.first(),
            Some(&"bundles 登记（dsh-base + web-app）".to_string())
        );
        assert!(outcome.warnings.is_empty());
        // 安装层补齐；bridge 进 bundles 由 harness 的 reconcile 自动追加（真实 add/install 时）。
        assert_eq!(
            bundles_of(&profile),
            vec!["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]
        );
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert!(log.contains("DSHdesk dshmarket@1.31.1"));
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
            r#"{"dependencies":{},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let mut opts = init_options(&profile, &stub);
        opts.bridge_spec = "@cjiaojiao/dsh-desk-bridge@FAILME".into();
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.warnings.len() == 1);
        assert!(outcome.warnings[0].contains("FAILME"));
    }

    #[tokio::test]
    async fn web_app_在dependencies时迁移清理() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // 历史污染：web-app 被旧版初始化 add 进 dependencies（全家桶源头）。
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"0.1.0-rc.8","@cjiaojiao/dsh-desk-bridge":"0.1.0"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","@cjiaojiao/dsh-desk-bridge"]}}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.ran_adds.iter().any(|a| a.contains("依赖迁移")));
        // 迁移后：dependencies 不再有 web-app，bundles 保留安装层。
        assert!(!deps_of(&profile).contains(&"@deepseek-ai/dsh-web-app".to_string()));
        assert!(bundles_of(&profile).contains(&"@deepseek-ai/dsh-web-app".to_string()));
        // install 被调用过一次（重建 node_modules 清理全家桶）。
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert_eq!(
            log.trim()
                .lines()
                .filter(|l| l.ends_with(" install"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn 废弃包名_从bundles与dependencies清理() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // 改名前的 bridge 残留（duplicate loader entry id 事故源头）。
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{"@JiaosSir/dsh-desk-bridge":"0.1.0","@cjiaojiao/dsh-desk-bridge":"0.1.0"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","@JiaosSir/dsh-desk-bridge","@cjiaojiao/dsh-desk-bridge"]}}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.ran_adds.iter().any(|a| a.contains("依赖迁移")));
        assert!(!deps_of(&profile).contains(&"@JiaosSir/dsh-desk-bridge".to_string()));
        assert!(!bundles_of(&profile).contains(&"@JiaosSir/dsh-desk-bridge".to_string()));
        assert!(bundles_of(&profile).contains(&"@cjiaojiao/dsh-desk-bridge".to_string()));
        let log = fs::read_to_string(tmp.path().join("adds.log")).unwrap();
        assert_eq!(
            log.trim()
                .lines()
                .filter(|l| l.ends_with(" install"))
                .count(),
            1
        );
    }

    #[test]
    fn 用户层patch_幂等_已有配置不重复写() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // 用户已手写 dsh-market 配置（带自定义注释）：不得改写。
        let existing = "# 用户自定义\n- id: dsh-market\n  config:\n    allowRestart: false\n";
        fs::write(profile.join("cordis.patch.yml"), existing).unwrap();
        assert!(!ensure_user_patch_overrides(&profile).unwrap());
        let text = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert_eq!(text, existing);
    }

    #[test]
    fn 用户层patch_空数组形态整体替换() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // harness 默认模板：注释 + `[]`。追加行会让 YAML 非法，必须整体替换。
        fs::write(
            profile.join("cordis.patch.yml"),
            "# dsh profile root — an empty entry list.\n[]\n",
        )
        .unwrap();
        assert!(ensure_user_patch_overrides(&profile).unwrap());
        let text = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(text.contains("- id: dsh-market"));
        assert!(text.contains("allowRestart: false"));
        assert!(!text.contains("[]"));
    }

    #[test]
    fn 用户层patch_不存在时创建() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        assert!(ensure_user_patch_overrides(&profile).unwrap());
        let text = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(text.contains("- id: dsh-market"));
        assert!(text.contains("profile: DSHdesk"));
        // 缩进必须完整：`config:` 是 `- id:` 的子键（两空格），缺失会使 YAML 解析失败（实测事故）。
        assert!(text.contains("\n  config:\n"));
        assert!(text.contains("\n    allowRestart: false\n"));
        // 再次调用：幂等。
        assert!(!ensure_user_patch_overrides(&profile).unwrap());
    }

    #[test]
    fn 用户层patch_损坏版本_整体替换() {
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        // 早期 bug 生成的缩进损坏文件：`config:` 无缩进（顶层键），YAML 非法，
        // 虽含 `allowRestart: false` 字样也必须重写。
        fs::write(
            profile.join("cordis.patch.yml"),
            "# DSH-desk 生成的用户层 patch：默认插件的桌面环境配置。\n# dsh-market：桌面由壳监督宿主生命周期，禁用市场自带重启（防孤儿进程）。\n- id: dsh-market\nconfig:\nallowRestart: false\nprofile: DSHdesk\n",
        )
        .unwrap();
        assert!(ensure_user_patch_overrides(&profile).unwrap());
        let text = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(text.contains("\n  config:\n"));
        assert!(text.contains("\n    allowRestart: false\n"));
        assert!(!text.contains("\nconfig:\n"));
    }

    #[tokio::test]
    async fn 默认插件_add失败_不阻塞() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let mut ran_adds = Vec::new();
        let mut warnings = Vec::new();
        ensure_default_plugins(
            &opts,
            &["@fake/market@FAILME"],
            &mut ran_adds,
            &mut warnings,
        )
        .await;
        assert!(ran_adds.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("FAILME"));
        assert!(warnings[0].contains("不阻塞启动"));
    }

    #[tokio::test]
    async fn 默认插件_依赖含deepseek核心包时warning() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profile = tmp.path().join("desktop");
        fs::create_dir_all(&profile).unwrap();
        fs::write(
            profile.join("package.json"),
            r#"{"dependencies":{},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();
        // add 成功后（stub 不建目录，手动模拟）插件携带 @deepseek-ai 依赖。
        let pkg_dir = profile.join("node_modules").join("bad-market");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(
            pkg_dir.join("package.json"),
            r#"{"name":"bad-market","version":"1.0.0","dependencies":{"@deepseek-ai/dsh-tools":"0.1.1-rc.2","js-yaml":"^4.1.0"}}"#,
        )
        .unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profile, &stub);
        let mut ran_adds = Vec::new();
        let mut warnings = Vec::new();
        ensure_default_plugins(&opts, &["bad-market@1.0.0"], &mut ran_adds, &mut warnings).await;
        assert_eq!(ran_adds, vec!["bad-market@1.0.0".to_string()]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("双副本"));
        assert!(warnings[0].contains("@deepseek-ai/dsh-tools"));
    }

    #[tokio::test]
    async fn 旧profile名_自动迁移() {
        if !node_available() {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let profiles = tmp.path().join("profiles");
        // 旧名目录存在（含配置），新名目录不存在。
        let legacy = profiles.join(LEGACY_PROFILE_NAME);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("package.json"), "{}").unwrap();
        let stub = write_stub(&tmp);
        let opts = init_options(&profiles.join(PROFILE_NAME), &stub);
        let outcome = ensure_profile_init(&opts).await.unwrap();
        assert!(outcome.ran_adds.iter().any(|a| a.contains("profile 迁移")));
        assert!(outcome
            .ran_adds
            .iter()
            .any(|a| a.contains("desktop → DSHdesk")));
        assert!(!legacy.exists());
        assert!(profiles.join(PROFILE_NAME).join("package.json").exists());
    }

    #[test]
    fn 旧profile名_新名已存在时不迁移() {
        let tmp = TempDir::new().unwrap();
        let profiles = tmp.path().join("profiles");
        let legacy = profiles.join(LEGACY_PROFILE_NAME);
        let current = profiles.join(PROFILE_NAME);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        assert!(migrate_legacy_profile(&current).unwrap().is_none());
        assert!(legacy.exists(), "新名已存在时不得改名旧目录");
    }
}
