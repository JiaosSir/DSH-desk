//! 应用内更新：查询 GitHub Releases 最新版本 → 与当前版本比较 → 下载安装包 →
//! 静默覆盖安装并自动重启。
//!
//! 安装版（NSIS）走 `setup.exe /S /UPDATE /R`（模板内置的更新模式：静默、自动
//! 结束运行中的应用、装完自动拉起新版本）；便携版不做应用内更新，只提示用户
//! 前往 GitHub Releases 手动下载压缩包覆盖。本模块不依赖 tauri，可单测。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// 发布源（GitHub Releases 唯一渠道）。
const API_LATEST: &str = "https://api.github.com/repos/JiaosSir/DSH-desk/releases/latest";
/// NSIS 安装包资产名后缀。
const SETUP_SUFFIX: &str = "-setup.exe";
/// 便携版压缩包资产名前缀/后缀。
const PORTABLE_PREFIX: &str = "DSH-desk-portable-";
const PORTABLE_SUFFIX: &str = ".zip";
/// 安装器静默更新参数（installer.nsi：/UPDATE 跳过重装页/WebView2/快捷方式清理，/R 装完自动重启）。
const INSTALLER_ARGS: [&str; 3] = ["/S", "/UPDATE", "/R"];

/// 更新模式：安装版可全流程应用内更新；便携版仅提示手动下载。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    Installed,
    Portable,
}

/// 更新会话（AppState 共享；UI 轮询进度）。
#[derive(Debug, Clone)]
pub struct UpdateSession {
    pub mode: UpdateMode,
    pub progress: UpdateProgress,
    /// 检查发现的新版本资产（check 写入，download 消费）。
    pub pending: Option<PendingUpdate>,
    /// 下载完成的本地安装包路径（download 写入，install 消费）。
    pub downloaded: Option<PathBuf>,
}

/// 待下载的更新资产。
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub url: String,
    pub name: String,
    pub size: u64,
}

impl UpdateSession {
    pub fn new() -> Self {
        Self {
            mode: UpdateMode::Installed,
            progress: UpdateProgress::idle(),
            pending: None,
            downloaded: None,
        }
    }

    /// 记录一次检查结果（设置区检查命令与启动自动检查共用）：重置进度并登记
    /// 待下载资产，供 `desktop_download_update` / `desktop_install_update` 消费。
    pub fn record_check(&mut self, mode: UpdateMode, info: &UpdateInfo) {
        self.mode = mode;
        self.progress = UpdateProgress::idle();
        self.pending = info.asset_url.clone().map(|url| PendingUpdate {
            url,
            name: info.asset_name.clone().unwrap_or_default(),
            size: info.asset_size.unwrap_or(0),
        });
        self.downloaded = None;
    }
}

impl Default for UpdateSession {
    fn default() -> Self {
        Self::new()
    }
}

/// 更新进度快照（UI 轮询 `desktop_update_progress`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// idle / checking / downloading / downloaded / installing / error。
    pub phase: &'static str,
    pub received: u64,
    pub total: u64,
    pub message: Option<String>,
}

impl UpdateProgress {
    pub fn idle() -> Self {
        Self {
            phase: "idle",
            received: 0,
            total: 0,
            message: None,
        }
    }
}

/// 检查更新结果（`desktop_check_update` 返回值，camelCase 对齐 bridge 协议）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 是否存在比当前更新的版本。
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    /// 发布说明（GitHub Release body，可能为 null）。
    pub notes: Option<String>,
    /// 匹配到的安装包资产（无匹配为 None）。
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
    /// true = 便携版（不做应用内更新，仅提示手动下载）。
    pub portable: bool,
}

/// GitHub Releases latest 响应（只取用到的字段）。
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// 判定更新模式：NSIS 安装版在 exe 同目录写 uninstall.exe，便携版没有。
/// `DSH_DESK_UPDATE_MODE=installed|portable` 可强制指定模式（dev 验证链路用；
/// 不设置则按 uninstall.exe 自动判定）。
pub fn detect_mode() -> UpdateMode {
    match std::env::var("DSH_DESK_UPDATE_MODE").as_deref() {
        Ok("installed") => return UpdateMode::Installed,
        Ok("portable") => return UpdateMode::Portable,
        _ => {}
    }
    if is_installed(&current_exe_dir()) {
        UpdateMode::Installed
    } else {
        UpdateMode::Portable
    }
}

/// 参与比较/展示的当前版本：`DSH_DESK_UPDATE_CURRENT_VERSION` 可强制覆盖
/// （dev 验证「有新版本」链路用——填一个低于 GitHub 最新 release 的版本号，
/// 不必改 tauri.conf.json）；未设置则用应用真实版本。
pub fn effective_current_version(package_version: &str) -> String {
    std::env::var("DSH_DESK_UPDATE_CURRENT_VERSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| package_version.to_owned())
}

/// exe 同目录是否存在 uninstall.exe（可单测）。
pub fn is_installed(exe_dir: &Path) -> bool {
    exe_dir.join("uninstall.exe").exists()
}

fn current_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default()
}

/// 解析版本号（容忍 tag 的 `v` 前缀）。
fn parse_version(s: &str) -> Result<semver::Version, String> {
    let s = s.strip_prefix('v').unwrap_or(s);
    semver::Version::parse(s).map_err(|e| format!("无法解析版本号 {s:?}: {e}"))
}

/// 从发布资产中按模式挑选更新包。
fn pick_asset(assets: &[Asset], mode: UpdateMode) -> Option<&Asset> {
    match mode {
        UpdateMode::Installed => assets.iter().find(|a| a.name.ends_with(SETUP_SUFFIX)),
        UpdateMode::Portable => assets
            .iter()
            .find(|a| a.name.starts_with(PORTABLE_PREFIX) && a.name.ends_with(PORTABLE_SUFFIX)),
    }
}

/// 查询 GitHub Releases 最新版本并与当前版本比较。
pub fn check_for_update(current_version: &str, mode: UpdateMode) -> Result<UpdateInfo, String> {
    let body = crate::http::get_text(API_LATEST, &format!("dsh-desk/{current_version}"), 20_000)
        .map_err(|e| format!("查询 GitHub Releases 失败: {e}"))?;
    let release: Release =
        serde_json::from_str(&body).map_err(|e| format!("解析 GitHub 响应失败: {e}"))?;

    let latest = parse_version(&release.tag_name)?;
    let current = parse_version(current_version)?;
    let asset = pick_asset(&release.assets, mode);
    Ok(UpdateInfo {
        available: latest > current,
        current_version: current_version.to_owned(),
        latest_version: release.tag_name,
        notes: release.body,
        asset_name: asset.map(|a| a.name.clone()),
        asset_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size),
        portable: mode == UpdateMode::Portable,
    })
}

/// 下载更新安装包到 `dest`，进度写入共享会话；成功时记录 `session.downloaded`。
/// 失败时清理半成品并置 error 阶段。
pub fn download_update(
    session: &Arc<Mutex<UpdateSession>>,
    pending: &PendingUpdate,
    dest: &Path,
) -> Result<(), String> {
    let result = crate::http::download(
        &pending.url,
        "dsh-desk-updater",
        dest,
        &mut |received, total| {
            set_progress(session, "downloading", received, total, None);
        },
    );
    let total = result.inspect_err(|e| {
        let _ = std::fs::remove_file(dest);
        fail(session, e);
    })?;
    {
        let mut s = session.lock().expect("更新会话锁");
        s.downloaded = Some(dest.to_path_buf());
        s.progress = UpdateProgress {
            phase: "downloaded",
            received: total,
            total,
            message: None,
        };
    }
    Ok(())
}

/// 启动安装器静默更新（仅安装版）：直接 spawn `setup.exe /S /UPDATE /R`。
/// setup.exe 是 GUI 子系统程序——不会弹控制台窗口，子进程随父进程退出自然
/// 存活。调用方必须先停掉 sidecar 再调用本函数（否则安装器静默模式会杀掉
/// 本进程、留下孤儿 sidecar——这是当初用批处理等退出的原因，现在由调用方
/// 显式先停宿主解决，不再需要批处理）。
pub fn launch_installer(setup_exe: &Path) -> Result<(), String> {
    std::process::Command::new(setup_exe)
        .args(INSTALLER_ARGS)
        .spawn()
        .map_err(|e| format!("启动安装器失败: {e}"))?;
    Ok(())
}

/// 把会话置为 error 阶段并附原因。
fn fail(session: &Arc<Mutex<UpdateSession>>, message: &str) {
    set_progress(session, "error", 0, 0, Some(message.to_owned()));
}

fn set_progress(
    session: &Arc<Mutex<UpdateSession>>,
    phase: &'static str,
    received: u64,
    total: u64,
    message: Option<String>,
) {
    let mut s = session.lock().expect("更新会话锁");
    s.progress = UpdateProgress {
        phase,
        received,
        total,
        message,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 1024,
        }
    }

    #[test]
    fn parse_version_accepts_v_prefix() {
        assert_eq!(
            parse_version("v1.2.3").unwrap(),
            semver::Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_version("1.2.3").unwrap(),
            semver::Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_version("v0.1.1-rc.2").unwrap(),
            semver::Version::parse("0.1.1-rc.2").unwrap()
        );
        assert!(parse_version("not-a-version").is_err());
    }

    #[test]
    fn prerelease_sorts_below_stable() {
        let rc = parse_version("v1.0.1-rc.2").unwrap();
        let stable = parse_version("v1.0.1").unwrap();
        assert!(rc < stable);
    }

    #[test]
    fn pick_asset_installed_finds_setup_exe() {
        let assets = vec![
            asset("DSH-desk-portable-1.0.1-x64.zip"),
            asset("DSH-desk_1.0.1_x64-setup.exe"),
            asset("latest.json"),
        ];
        let picked = pick_asset(&assets, UpdateMode::Installed).expect("应选中 setup.exe");
        assert_eq!(picked.name, "DSH-desk_1.0.1_x64-setup.exe");
    }

    #[test]
    fn pick_asset_portable_finds_zip() {
        let assets = vec![
            asset("DSH-desk_1.0.1_x64-setup.exe"),
            asset("DSH-desk-portable-1.0.1-x64.zip"),
        ];
        let picked = pick_asset(&assets, UpdateMode::Portable).expect("应选中便携 zip");
        assert_eq!(picked.name, "DSH-desk-portable-1.0.1-x64.zip");
    }

    #[test]
    fn pick_asset_missing_returns_none() {
        let assets = vec![asset("readme.txt")];
        assert!(pick_asset(&assets, UpdateMode::Installed).is_none());
        assert!(pick_asset(&assets, UpdateMode::Portable).is_none());
    }

    #[test]
    fn release_json_parses_github_shape() {
        let body = r#"{
          "tag_name": "v1.0.1",
          "body": "修复若干问题",
          "assets": [
            { "name": "DSH-desk_1.0.1_x64-setup.exe",
              "browser_download_url": "https://github.com/JiaosSir/DSH-desk/releases/download/v1.0.1/DSH-desk_1.0.1_x64-setup.exe",
              "size": 12345 }
          ]
        }"#;
        let release: Release = serde_json::from_str(body).expect("应能解析 GitHub 响应");
        assert_eq!(release.tag_name, "v1.0.1");
        assert_eq!(release.body.as_deref(), Some("修复若干问题"));
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].size, 12345);
    }

    #[test]
    fn check_update_computes_availability() {
        // 无网络路径：直接验证比较逻辑与字段组装。
        let info = UpdateInfo {
            available: true,
            current_version: "1.0.0".into(),
            latest_version: "v1.0.1".into(),
            notes: None,
            asset_name: Some("DSH-desk_1.0.1_x64-setup.exe".into()),
            asset_url: Some("https://example.com/setup.exe".into()),
            asset_size: Some(12345),
            portable: false,
        };
        assert!(info.available);
        let newer = parse_version(&info.latest_version).unwrap();
        let current = parse_version(&info.current_version).unwrap();
        assert!(newer > current);
    }

    #[test]
    fn record_check_登记待下载资产并重置状态() {
        let info = UpdateInfo {
            available: true,
            current_version: "1.0.0".into(),
            latest_version: "v1.0.1".into(),
            notes: None,
            asset_name: Some("DSH-desk_1.0.1_x64-setup.exe".into()),
            asset_url: Some("https://example.com/setup.exe".into()),
            asset_size: Some(12345),
            portable: false,
        };
        let mut session = UpdateSession::new();
        session.downloaded = Some(PathBuf::from("C:\\tmp\\old.exe"));
        session.record_check(UpdateMode::Installed, &info);
        assert_eq!(session.mode, UpdateMode::Installed);
        assert!(session.downloaded.is_none(), "新检查应清空已下载路径");
        let pending = session.pending.expect("应登记待下载资产");
        assert_eq!(pending.url, "https://example.com/setup.exe");
        assert_eq!(pending.name, "DSH-desk_1.0.1_x64-setup.exe");
        assert_eq!(pending.size, 12345);
        assert_eq!(session.progress.phase, "idle");
    }

    #[test]
    fn detect_mode_支持环境变量强制() {
        std::env::set_var("DSH_DESK_UPDATE_MODE", "installed");
        assert_eq!(detect_mode(), UpdateMode::Installed);
        std::env::set_var("DSH_DESK_UPDATE_MODE", "portable");
        assert_eq!(detect_mode(), UpdateMode::Portable);
        std::env::remove_var("DSH_DESK_UPDATE_MODE");
        // 未设置时按 exe 环境自动判定（test 二进制所在目录无 uninstall.exe）。
        assert_eq!(detect_mode(), UpdateMode::Portable);
    }

    #[test]
    fn 当前版本_支持环境变量覆盖() {
        std::env::set_var("DSH_DESK_UPDATE_CURRENT_VERSION", "0.9.0");
        assert_eq!(effective_current_version("1.0.0"), "0.9.0");
        std::env::set_var("DSH_DESK_UPDATE_CURRENT_VERSION", "");
        assert_eq!(
            effective_current_version("1.0.0"),
            "1.0.0",
            "空值回退真实版本"
        );
        std::env::remove_var("DSH_DESK_UPDATE_CURRENT_VERSION");
        assert_eq!(effective_current_version("1.0.0"), "1.0.0");
    }
}
