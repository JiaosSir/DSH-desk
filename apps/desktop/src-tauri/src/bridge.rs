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
  checkUpdate: () => window.__TAURI__.core.invoke('desktop_check_update'),
  downloadUpdate: () => window.__TAURI__.core.invoke('desktop_download_update'),
  getUpdateProgress: () => window.__TAURI__.core.invoke('desktop_update_progress'),
  installUpdate: () => window.__TAURI__.core.invoke('desktop_install_update'),
  restartHost: () => window.__TAURI__.core.invoke('desktop_retry'),
  setAutostart: (enabled) => window.__TAURI__.core.invoke('desktop_set_autostart', { enabled }),
  getAutostart: () => window.__TAURI__.core.invoke('desktop_get_autostart'),
  getTitlebarMode: () => window.__TAURI__.core.invoke('desktop_get_titlebar_mode'),
  setTitlebarMode: (mode) => window.__TAURI__.core.invoke('desktop_set_titlebar_mode', { mode }),
  quit: () => window.__TAURI__.core.invoke('desktop_quit'),
  notify: (title, body) => window.__TAURI__.core.invoke('desktop_notify', { title, body }),
};
// 页面类别上报：资产页 = asset、sidecar 页 = host，壳据此决定导航方式；纯浏览器静默忽略。
try {
  window.__TAURI__.core.invoke('desktop_page_kind', {
    kind: location.hostname === '127.0.0.1' ? 'host' : 'asset',
  }).catch(() => {});
} catch (e) { /* ignore */ }

// 自绘透明标题栏（原生标题栏隐藏时）：body 顶部下推 TITLEBAR_HEIGHT 露出
// 一条与 DSH 主题背景同色的透明拖拽区（fixed 条带 data-tauri-drag-region），
// 高度与原生标题栏一致；纯浏览器/原生模式下降级为空操作。
// 锚定不依赖 DSH 内部 DOM（不动 centerCol/grid 布局），只改 body 的
// padding-top 与 box-sizing；恢复时原样还原，不影响其它壳注入。
(function () {
  var HOST_ONLY = location.hostname === '127.0.0.1';
  var TB_ID = 'dshDeskTitlebar';
  var TB_HEIGHT = 32; // 与原生标题栏高度一致（Windows 标准 32px）

  function applyTitlebar() {
    if (!HOST_ONLY) return;
    if (document.getElementById(TB_ID)) return;
    // box-sizing: border-box 保证 padding-top 后 #root 高度仍为 100% - 32px，
    // 内容整体下移、不被截断（html/body/#root 高度链见 dsh base.css）。
    document.body.style.boxSizing = 'border-box';
    document.body.style.paddingTop = TB_HEIGHT + 'px';
    var bar = document.createElement('div');
    bar.id = TB_ID;
    bar.setAttribute('data-tauri-drag-region', '');
    bar.style.cssText = 'position:fixed;top:0;left:0;right:0;height:' + TB_HEIGHT +
      'px;z-index:2147483647;background:transparent;';
    document.body.appendChild(bar);
  }

  function removeTitlebar() {
    var bar = document.getElementById(TB_ID);
    if (bar) bar.remove();
    document.body.style.paddingTop = '';
    document.body.style.boxSizing = '';
  }

  function initTitlebar() {
    try {
      window.__TAURI__.event.listen('desktop_titlebar', function (e) {
        if (e.payload === 'hidden') applyTitlebar(); else removeTitlebar();
      }).catch(function () {});
      window.__DSH_DESK__.getTitlebarMode().then(function (m) {
        if (m === 'hidden') applyTitlebar(); else removeTitlebar();
      }).catch(function () {});
    } catch (e) { /* ignore */ }
  }

  // initialization script 执行时 DOM 可能尚未就绪；等 DOMContentLoaded 再挂。
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initTitlebar);
  } else {
    initTitlebar();
  }
})();

// 宿主页历史护栏：压入占位条目，返回键时把用户推回当前视图
// （WebView 返回键会落回首次加载条目，重新触发 dsh 启动 loading）。
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

/// 启动自动检查发现新版本时，注入到宿主页侧边栏的「下载更新」横幅脚本。
///
/// 锚定方式遵循 dsh 生态实践（dsh-web-dev `semantic-attrs-v1.md` 的兼容适配器
/// 思路）：官方侧边栏当前没有 logo 与「新建会话」之间的 slot，也没有稳定的
/// 语义属性，因此用**类名包含匹配** `[class*="logoRow"]` / `button[class*="newSession"]`
/// ——CSS Module 哈希前缀（`hHd-Xa_` 之类）会随构建变化，但本地名
/// `logoRow`/`newSession` 是该组件的稳定语义片段，升级 dsh 引擎后无需改动。
/// 样式只用主题令牌（`--dsw-alias-*`），明暗皮肤/换肤自动适配，不写死颜色。
/// 横幅容器与部件输出 `data-dsh-plugin` / `data-dsh-part` 语义属性（L2 契约
/// 约定），按钮复用 `window.__DSH_DESK__` 的 downloadUpdate / getUpdateProgress /
/// installUpdate；右上角 × 关闭（本次会话内不再显示）。
pub fn update_banner_script(latest_version: &str, size: u64) -> String {
    let script = r#"(function () {
  if (!window.__DSH_DESK__) return;
  var LOGO = '[class*="logoRow"]';
  var NEW_SESSION = 'button[class*="newSession"]';
  var LATEST = __LATEST_JSON__;
  var SIZE = __SIZE__;
  var tries = 0;
  var timer = setInterval(function () {
    tries++;
    var logoRow = document.querySelector(LOGO);
    var newSession = document.querySelector(NEW_SESSION);
    if (logoRow && newSession && logoRow.parentElement === newSession.parentElement) {
      clearInterval(timer);
      buildBanner(logoRow.parentElement, newSession);
    } else if (tries >= 60) {
      clearInterval(timer);
    }
  }, 500);
  function sizeText(n) {
    if (!n) return '';
    var mb = n / 1024 / 1024;
    return mb >= 1024 ? '（' + (mb / 1024).toFixed(1) + ' GB）' : '（' + mb.toFixed(1) + ' MB）';
  }
  function buildBanner(parent, anchor) {
    if (document.getElementById('dshDeskUpdateBanner')) return;
    var wrap = document.createElement('div');
    wrap.id = 'dshDeskUpdateBanner';
    wrap.setAttribute('data-dsh-plugin', 'dsh-desk-update');
    wrap.style.cssText = 'position:relative;margin:0 0 8px;padding:6px;' +
      'border-radius:8px;' +
      'background:var(--dsw-alias-fill-tsp-secondary);color:var(--dsw-alias-label-primary);font-size:12px;';
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.setAttribute('data-dsh-part', 'download');
    // 下载按钮占满整宽：不为关闭钮预留内边距（关闭钮绝对定位悬浮在右上角）。
    // 版本文案不加前缀：latest 来自 GitHub tag（自带 v）。
    btn.style.cssText = 'box-sizing:border-box;width:100%;padding:6px 10px;border-radius:6px;cursor:pointer;' +
      'border:1px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-button-primary-fill);' +
      'color:var(--dsw-alias-label-primary-foreground);font-size:12px;white-space:nowrap;';
    btn.textContent = '下载更新 ' + LATEST + sizeText(SIZE);
    var close = document.createElement('button');
    close.type = 'button';
    close.setAttribute('aria-label', '关闭更新提示');
    close.setAttribute('data-dsh-part', 'close');
    close.title = '关闭';
    close.textContent = '×';
    // 右上角绝对定位的圆形填充按钮，悬浮于横幅角上；底色用应用底色
    // （--dsw-alias-bg-base，明暗主题均与主按钮填充形成高对比，避免被按钮吞掉）。
    close.style.cssText = 'position:absolute;top:4px;right:4px;width:18px;height:18px;' +
      'line-height:16px;padding:0;border-radius:50%;box-sizing:border-box;' +
      'border:1px solid var(--dsw-alias-border-l2);background:var(--dsw-alias-bg-base);' +
      'color:var(--dsw-alias-label-primary);opacity:.95;cursor:pointer;font-size:13px;text-align:center;';
    close.addEventListener('click', function () { wrap.remove(); });
    btn.addEventListener('click', function () {
      var d = window.__DSH_DESK__;
      if (btn.dataset.step === 'install') { d.installUpdate().catch(function () {}); return; }
      btn.disabled = true;
      btn.textContent = '下载中…';
      var poll = setInterval(function () {
        d.getUpdateProgress().then(function (p) {
          if (p.phase === 'downloaded') {
            clearInterval(poll);
            btn.disabled = false;
            btn.dataset.step = 'install';
            btn.textContent = '立即安装并重启';
          } else if (p.phase === 'error') {
            clearInterval(poll);
            btn.disabled = false;
            btn.textContent = '下载失败，重试';
          } else if (p.total > 0) {
            btn.textContent = '下载中 ' + Math.min(100, Math.round(p.received / p.total * 100)) + '%';
          }
        }).catch(function () {});
      }, 300);
      d.downloadUpdate().catch(function () {
        clearInterval(poll);
        btn.disabled = false;
        btn.textContent = '下载失败，重试';
      });
    });
    wrap.appendChild(btn);
    wrap.appendChild(close);
    parent.insertBefore(wrap, anchor);
  }
})();"#;
    script
        .replace(
            "__LATEST_JSON__",
            &serde_json::to_string(latest_version).unwrap_or_else(|_| "\"\"".to_owned()),
        )
        .replace("__SIZE__", &size.to_string())
}

#[cfg(test)]
mod tests {
    use super::{update_banner_script, BRIDGE_SCRIPT};

    #[test]
    fn 横幅脚本用包含匹配锚点与主题令牌() {
        let js = update_banner_script("v1.2.3", 47_185_920);
        // 锚点：CSS Module 本地名包含匹配（哈希前缀变化不失效）。
        assert!(js.contains(r#"[class*="logoRow"]"#));
        assert!(js.contains(r#"button[class*="newSession"]"#));
        assert!(!js.contains("hHd-Xa_"), "不得依赖构建哈希前缀");
        // 样式：只用主题令牌，不写死颜色。
        assert!(js.contains("var(--dsw-alias-button-primary-fill)"));
        assert!(js.contains("var(--dsw-alias-label-primary-foreground)"));
        assert!(js.contains("var(--dsw-alias-border-l2)"));
        assert!(js.contains("var(--dsw-alias-fill-tsp-secondary)"));
        assert!(!js.contains("rgba("), "不得写死颜色");
        // 关闭钮：右上角绝对定位的圆形填充钮（应用底色高对比）；下载按钮占满整宽。
        assert!(js.contains("border-radius:50%"), "关闭钮应为圆形");
        assert!(js.contains("box-sizing:border-box"), "按钮应含 box-sizing");
        assert!(
            js.contains("background:var(--dsw-alias-bg-base)"),
            "关闭钮用应用底色"
        );
        assert_eq!(
            js.matches("var(--dsw-alias-border-l2)").count(),
            2,
            "外边框已去掉：仅下载按钮与关闭钮各保留一个边框"
        );
        // 版本文案：latest 自带 v 前缀，不再拼接 'v'。
        assert!(js.contains("'下载更新 ' + LATEST"), "不应再加 v 前缀");
        assert!(!js.contains("'下载更新 v' + LATEST"), "不得重复 v 前缀");
        // 语义属性（L2 契约）：容器与部件锚点。
        assert!(js.contains(r#"data-dsh-plugin"#));
        assert!(js.contains(r#"data-dsh-part"#));
        assert!(js.contains("dshDeskUpdateBanner"));
        assert!(js.contains("\"v1.2.3\""));
        assert!(js.contains("47185920"), "size 应内嵌为字面量");
        assert!(js.contains("MB）"), "应有体积文案模板");
        assert!(js.contains("installUpdate"));
        assert!(!js.contains("__LATEST_JSON__"), "占位符应被替换");
        assert!(!js.contains("__SIZE__"), "占位符应被替换");
    }

    #[test]
    fn 横幅脚本版本串安全转义() {
        // 版本串里出现引号/反斜杠也不得破坏脚本（走 serde_json 转义）。
        let js = update_banner_script("v1\"x\\y", 0);
        assert!(js.contains("\\\""));
        assert!(!js.contains("__LATEST_JSON__"), "占位符应被替换");
    }

    #[test]
    fn 标题栏桥方法与透明条注入契约() {
        let js = BRIDGE_SCRIPT;
        // 桥方法：读/写标题栏模式（与 commands.rs 命令名一致）。
        assert!(js.contains(
            "getTitlebarMode: () => window.__TAURI__.core.invoke('desktop_get_titlebar_mode')"
        ));
        assert!(js.contains("setTitlebarMode: (mode) => window.__TAURI__.core.invoke('desktop_set_titlebar_mode', { mode })"));
        // 自绘透明条：全透明 + fixed 顶部 + data-tauri-drag-region（可拖拽）。
        assert!(js.contains("data-tauri-drag-region"));
        assert!(js.contains("background:transparent"), "标题栏必须全透明");
        assert!(
            js.contains("position:fixed;top:0;left:0;right:0"),
            "全宽覆盖窗口顶部"
        );
        assert!(
            js.contains("TB_HEIGHT = 32"),
            "高度与原生标题栏一致（32px）"
        );
        // 主题联动：透明条不写死颜色，露出 DSH 主题背景。
        assert!(!js.contains("background:#"), "不得写死标题栏颜色");
        assert!(
            js.contains("'border-box'"),
            "内容下推需保持高度链（boxSizing）"
        );
        assert!(
            js.contains("paddingTop = TB_HEIGHT + 'px'"),
            "body 顶部下推"
        );
        // 事件驱动重建：监听 desktop_titlebar 事件 + 启动时读当前模式。
        assert!(js.contains("desktop_titlebar"));
        assert!(js.contains("getTitlebarMode()"), "启动时恢复当前模式");
        // 纯浏览器/资产页降级：host 页才生效，事件/查询失败静默。
        assert!(js.contains("HOST_ONLY = location.hostname === '127.0.0.1'"));
        assert!(js.contains("/* ignore */"));
    }
}
