/**
 * 特性检测 + Tauri IPC 协议面——Web UI 与桌面壳之间唯一的桥接面。
 * 每个方法与 apps/desktop/src-tauri/src/commands.rs 里的 #[tauri::command]
 * 一一对应；壳经 initialization script 把具体对象注入为
 * window.__DSH_DESK__。纯浏览器里该全局对象不存在，所有调用方必须经
 * detectBridge 优雅退化。
 * @module @cjiaojiao/dsh-desk-bridge/client/bridge
 */

/** Tauri 壳注入的桌面桥协议。 */
export interface DeskBridge {
  /** 每个方法都由壳背书时为 true。 */
  readonly available: boolean
  /** 当前全局快捷键（只读展示）。 */
  getHotkey(): Promise<string>
  /** 在资源管理器中打开本地日志目录。 */
  openLogs(): Promise<void>
  /** 在系统浏览器中打开 GitHub Releases 页面。 */
  openReleases(): Promise<void>
  /** 仅重启 harness 宿主（sidecar）。 */
  restartHost(): Promise<void>
  /** 开/关开机自启；返回持久化后的状态。 */
  setAutostart(enabled: boolean): Promise<boolean>
  /** 当前开机自启状态。 */
  getAutostart(): Promise<boolean>
  /** 退出桌面应用（先停 sidecar）。 */
  quit(): Promise<void>
  /** 镜像一条系统通知（审批事件、长任务）。 */
  notify(title: string, body: string): Promise<void>
}

declare global {
  interface Window {
    __DSH_DESK__?: DeskBridge
  }
}

/**
 * 特性检测：返回注入的桥；纯浏览器返回 null。
 * 所有桌面 UI 必须把 null 视为"退化为等价的纯浏览器行为"。
 */
export function detectBridge(): DeskBridge | null {
  return typeof window !== 'undefined' && window.__DSH_DESK__ !== undefined
    ? window.__DSH_DESK__
    : null
}
