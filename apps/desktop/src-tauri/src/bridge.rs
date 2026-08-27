//! `window.__DSH_DESK__` 桥注入：把壳侧 IPC 命令包装成桥接插件约定的
//! `DeskBridge` 协议（与 packages/dsh-desk-bridge 的接口逐字段对齐）。
//! 等待页/错误页（本地资源）与 sidecar 页面（远程 URL）都会执行此脚本；
//! 纯浏览器打开同一份 UI 时该全局对象不存在，桥接插件自动退化。

/// 注入脚本（initialization_script，随每次页面加载执行）。
pub const BRIDGE_SCRIPT: &str = r#"
window.__DSH_DESK__ = {
  available: true,
  getState: () => window.__TAURI__.core.invoke('desktop_state'),
  getHotkey: () => window.__TAURI__.core.invoke('desktop_get_hotkey'),
  openLogs: () => window.__TAURI__.core.invoke('desktop_open_logs'),
  openReleases: () => window.__TAURI__.core.invoke('desktop_open_releases'),
  restartHost: () => window.__TAURI__.core.invoke('desktop_retry'),
  setAutostart: (enabled) => window.__TAURI__.core.invoke('desktop_set_autostart', { enabled }),
  getAutostart: () => window.__TAURI__.core.invoke('desktop_get_autostart'),
  quit: () => window.__TAURI__.core.invoke('desktop_quit'),
  notify: (title, body) => window.__TAURI__.core.invoke('desktop_notify', { title, body }),
};
// 页面类别上报：本地资产页（等待页/错误页）= asset，sidecar 页面 = host。
// 壳据此决定由页面自行 location.replace 导航（历史干净）还是 webview.navigate
// 兜底；纯浏览器打开时 __TAURI__ 不存在，静默忽略。
try {
  window.__TAURI__.core.invoke('desktop_page_kind', {
    kind: location.hostname === '127.0.0.1' ? 'host' : 'asset',
  }).catch(() => {});
} catch (e) { /* ignore */ }

// 宿主页历史护栏：WebView 中返回键可能落回宿主页的首次加载条目，重新触发
// dsh 自己的启动 loading（网页版标签页没有前一条目，不会遇到）。压入一个
// 占位条目，popstate（返回键）时把用户推回当前视图，返回键彻底无感。
if (location.hostname === '127.0.0.1') {
  try {
    history.pushState({ dshGuard: true }, '', location.href);
    window.addEventListener('popstate', () => {
      if (history.state && history.state.dshGuard) return;
      history.pushState({ dshGuard: true }, '', location.href);
    });
  } catch (e) { /* ignore */ }
}
"#;
