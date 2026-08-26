//! DSH_HOME 解析与桌面自有路径布局。
//!
//! 与 harness 行为一致：`DSH_HOME` 环境变量优先（空/空白视为未设置），
//! 否则回退 `~/.dsh`（见 `packages/util/home-paths` 的 resolveDshHome）。

use std::ffi::OsStr;
use std::path::PathBuf;

/// 纯函数形式的 DSH_HOME 解析（可测，避免并发测试改环境变量）。
///
/// `env_home` = `DSH_HOME` 环境变量的原始值；`fallback_home` = 未设置时的
/// 回退目录（通常 `~/.dsh`）。
pub fn resolve_dsh_home(env_home: Option<&OsStr>, fallback_home: PathBuf) -> PathBuf {
    match env_home {
        Some(v) if !v.is_empty() && !v.to_string_lossy().trim().is_empty() => PathBuf::from(v),
        _ => fallback_home,
    }
}

/// 当前进程的 harness 主目录：`DSH_HOME` 优先，否则 `~/.dsh`。
pub fn dsh_home() -> PathBuf {
    let fallback = dirs::home_dir().expect("无法解析用户主目录").join(".dsh");
    resolve_dsh_home(std::env::var_os("DSH_HOME").as_deref(), fallback)
}

/// desktop profile 目录（`$DSH_HOME/profiles/DSHdesk`，名字见
/// `profile::PROFILE_NAME`，旧名 `desktop` 启动时自动迁移）。
pub fn profile_dir() -> PathBuf {
    dsh_home()
        .join("profiles")
        .join(crate::profile::PROFILE_NAME)
}

/// 桌面自有状态目录（config.json、日志等，规格 §8b）。
pub fn desktop_config_dir() -> PathBuf {
    dsh_home().join("desktop")
}

/// 日志目录（`$DSH_HOME/desktop/logs`，决策 D4）。
pub fn logs_dir() -> PathBuf {
    desktop_config_dir().join("logs")
}

/// sidecar 根目录：测试用 `SIDECAR_ROOT` 环境变量覆盖；
/// 生产路径由壳侧解析（Tauri 的 resource_dir）后传入。
pub fn sidecar_root() -> Option<PathBuf> {
    std::env::var_os("SIDECAR_ROOT").map(PathBuf::from)
}

/// 去掉 Windows verbatim 前缀（`\\?\`）。
///
/// Tauri 的 `resource_dir()` 在 Windows 上返回 `\\?\C:\...` 形态的路径；
/// node 用这种路径解析入口脚本时会把盘符段（如 `D:`）当文件 lstat，
/// 触发 `EISDIR` 直接崩溃（2026-08-25 实测：sidecar 秒退、监督器三连重试）。
/// 壳侧所有交给子进程的路径（node.exe / bin.js / pnpm 目录）都必须先过这里。
///
/// 实现用社区标准做法 `dunce::simplified`：qwen-code 桌面端在完全相同的
/// 架构下（Tauri 壳 + 自带 node 运行时）踩过同一坑，修复即换 dunce
/// （QwenLM/qwen-code#8936，起因 issue #8929）。盘符形态（`\\?\D:\...`）还原为
/// `D:\...`；扩展长度 UNC（`\\?\UNC\...`）按 dunce 设计保持原样（壳的
/// resource_dir 不会产生该形态）；其余平台原样返回。
pub fn deverbatim(path: &std::path::Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf {
        PathBuf::from(r"C:\Users\someone")
    }

    #[test]
    fn 设置_dsh_home_时优先() {
        let got = resolve_dsh_home(Some(OsStr::new(r"D:\data\dsh")), home().join(".dsh"));
        assert_eq!(got, PathBuf::from(r"D:\data\dsh"));
    }

    #[test]
    fn 空白_dsh_home_回退默认() {
        let fallback = home().join(".dsh");
        assert_eq!(
            resolve_dsh_home(Some(OsStr::new("")), fallback.clone()),
            fallback
        );
        assert_eq!(
            resolve_dsh_home(Some(OsStr::new("   ")), fallback.clone()),
            fallback
        );
        assert_eq!(resolve_dsh_home(None, fallback.clone()), fallback);
    }

    #[test]
    fn 子路径拼接正确() {
        // 直接验证纯函数式拼接，不依赖进程环境。
        let home = PathBuf::from(r"D:\data\dsh");
        assert_eq!(
            home.join("profiles").join(crate::profile::PROFILE_NAME),
            PathBuf::from(r"D:\data\dsh\profiles\DSHdesk")
        );
        assert_eq!(home.join("desktop"), PathBuf::from(r"D:\data\dsh\desktop"));
        assert_eq!(
            home.join("desktop").join("logs"),
            PathBuf::from(r"D:\data\dsh\desktop\logs")
        );
    }

    #[cfg(windows)]
    #[test]
    fn 去_verbatim_前缀() {
        // 壳的实际场景：resource_dir 返回 \\?\C:\... 的盘符形态，dunce 会还原。
        assert_eq!(
            deverbatim(std::path::Path::new(r"\\?\D:\app\sidecar-dist")),
            PathBuf::from(r"D:\app\sidecar-dist")
        );
        assert_eq!(
            deverbatim(std::path::Path::new(r"D:\plain\path")),
            PathBuf::from(r"D:\plain\path")
        );
        // 扩展长度 UNC（\\?\UNC\server\share）dunce 有意保持原样
        // （is_safe_to_strip_unc 只接受 VerbatimDisk；文档："leaves UNC paths as-is"）。
        // 壳的 resource_dir 不会产生该形态，此断言只是固化行为约定。
        assert_eq!(
            deverbatim(std::path::Path::new(r"\\?\UNC\server\share\dir")),
            PathBuf::from(r"\\?\UNC\server\share\dir")
        );
    }
}
