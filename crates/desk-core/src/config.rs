//! Desktop 自有状态（`~/.dsh/desktop/config.json`）：全局快捷键、开机自启、
//! 原生标题栏开关。读取失败一律回退默认值（fail-open，不阻塞启动），只有
//! 写入失败才报错。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

fn default_hotkey() -> String {
    "Ctrl+Alt+D".to_string()
}

/// 窗口标题栏模式：原生系统标题栏，或隐藏后由自绘透明标题栏替代。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitlebarMode {
    /// 原生标题栏（Windows 系统装饰）。
    #[default]
    Native,
    /// 隐藏原生标题栏；WebView 注入自绘透明标题栏（跟随 DSH 主题）。
    Hidden,
}

/// 桌面配置：快捷键（v1 只读展示）、开机自启、标题栏模式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopConfig {
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    pub autostart: bool,
    pub titlebar: TitlebarMode,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            autostart: false,
            titlebar: TitlebarMode::Native,
        }
    }
}

/// 配置文件路径（`<config_dir>/config.json`）。
pub fn config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.json")
}

/// 读配置：文件缺失或 JSON 非法 → 默认值；字段缺失 → 逐字段兜底。
pub fn load_from(config_dir: &Path) -> DesktopConfig {
    let path = config_path(config_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => DesktopConfig::default(),
    }
}

/// 写配置（自动创建父目录）。
pub fn save_to(config_dir: &Path, config: &DesktopConfig) -> Result<(), String> {
    let path = config_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("写入 {} 失败: {e}", path.display()))
}

/// 用当前 DSH_HOME 的桌面配置目录加载。
pub fn load() -> DesktopConfig {
    load_from(&paths::desktop_config_dir())
}

/// 用当前 DSH_HOME 的桌面配置目录保存。
pub fn save(config: &DesktopConfig) -> Result<(), String> {
    save_to(&paths::desktop_config_dir(), config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn 缺失文件返回默认值() {
        let tmp = TempDir::new().unwrap();
        let cfg = load_from(tmp.path());
        assert_eq!(
            cfg,
            DesktopConfig {
                hotkey: "Ctrl+Alt+D".to_string(),
                autostart: false,
                titlebar: TitlebarMode::Native,
            }
        );
    }

    #[test]
    fn 空对象逐字段兜底() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(config_path(tmp.path()), "{}").unwrap();
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.hotkey, "Ctrl+Alt+D");
        assert!(!cfg.autostart);
        assert_eq!(cfg.titlebar, TitlebarMode::Native);
    }

    #[test]
    fn 部分字段保留其余兜底() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(config_path(tmp.path()), r#"{"autostart":true}"#).unwrap();
        let cfg = load_from(tmp.path());
        assert!(cfg.autostart);
        assert_eq!(cfg.hotkey, "Ctrl+Alt+D");
        assert_eq!(cfg.titlebar, TitlebarMode::Native);
    }

    #[test]
    fn 标题栏模式读写() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(config_path(tmp.path()), r#"{"titlebar":"hidden"}"#).unwrap();
        let cfg = load_from(tmp.path());
        assert_eq!(cfg.titlebar, TitlebarMode::Hidden);
        // 非法枚举值回退默认（fail-open）。
        std::fs::write(config_path(tmp.path()), r#"{"titlebar":"weird"}"#).unwrap();
        assert_eq!(load_from(tmp.path()).titlebar, TitlebarMode::Native);
    }

    #[test]
    fn 标题栏模式序列化契约() {
        // 壳侧事件 payload / invoke 参数与桥脚本比较字符串，序列化必须是小写枚举名。
        assert_eq!(
            serde_json::to_string(&TitlebarMode::Native).unwrap(),
            "\"native\""
        );
        assert_eq!(
            serde_json::to_string(&TitlebarMode::Hidden).unwrap(),
            "\"hidden\""
        );
        assert_eq!(
            serde_json::from_str::<TitlebarMode>("\"hidden\"").unwrap(),
            TitlebarMode::Hidden
        );
    }

    #[test]
    fn 非法_json_回退默认() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(config_path(tmp.path()), "not json").unwrap();
        let cfg = load_from(tmp.path());
        assert_eq!(cfg, DesktopConfig::default());
    }

    #[test]
    fn 读写往返() {
        let tmp = TempDir::new().unwrap();
        let cfg = DesktopConfig {
            hotkey: "Ctrl+Shift+D".to_string(),
            autostart: true,
            titlebar: TitlebarMode::Hidden,
        };
        save_to(tmp.path(), &cfg).unwrap();
        assert_eq!(load_from(tmp.path()), cfg);
    }
}
