//! `window.__DSH_DESK__` 桥注入：把壳侧 IPC 命令包装成桥接插件约定的
//! `DeskBridge` 协议（与 packages/dsh-desk-bridge 的接口逐字段对齐）。
//! 等待页/错误页（本地资源）与 sidecar 页面（远程 URL）都会执行此脚本；
//! 纯浏览器打开同一份 UI 时该全局对象不存在，桥接插件自动退化。

/// 注入脚本（initialization_script，随每次页面加载执行）。
pub const BRIDGE_SCRIPT: &str = r#"
window.__DSH_DESK__ = {
  available: true,
  pickFolder: () => window.__TAURI__.core.invoke('desktop_pick_workspace'),
  openLogs: () => window.__TAURI__.core.invoke('desktop_open_logs'),
  openReleases: () => window.__TAURI__.core.invoke('desktop_open_releases'),
  restartHost: () => window.__TAURI__.core.invoke('desktop_retry'),
  setAutostart: (enabled) => window.__TAURI__.core.invoke('desktop_set_autostart', { enabled }),
  getAutostart: () => window.__TAURI__.core.invoke('desktop_get_autostart'),
  quit: () => window.__TAURI__.core.invoke('desktop_quit'),
  notify: (title, body) => window.__TAURI__.core.invoke('desktop_notify', { title, body }),
};
"#;
