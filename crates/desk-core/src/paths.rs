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

/// desktop profile 目录（`$DSH_HOME/profiles/desktop`）。
pub fn profile_dir() -> PathBuf {
    dsh_home().join("profiles").join("desktop")
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
            home.join("profiles").join("desktop"),
            PathBuf::from(r"D:\data\dsh\profiles\desktop")
        );
        assert_eq!(home.join("desktop"), PathBuf::from(r"D:\data\dsh\desktop"));
        assert_eq!(
            home.join("desktop").join("logs"),
            PathBuf::from(r"D:\data\dsh\desktop\logs")
        );
    }
}
