/**
 * 特性检测 + Tauri IPC 协议面——Web UI 与桌面壳之间唯一的桥接面：方法与
 * apps/desktop/src-tauri/src/commands.rs 的 #[tauri::command] 一一对应，壳经
 * initialization script 注入 window.__DSH_DESK__；纯浏览器中该对象不存在，
 * 所有调用方必须经 detectBridge 优雅退化。
 * @module @cjiaojiao/dsh-desk-bridge/client/bridge
 */

/** 检查更新结果（壳侧 desktop_check_update，camelCase）。 */
export interface UpdateInfo {
  /** 是否存在比当前更新的版本。 */
  available: boolean
  currentVersion: string
  latestVersion: string
  /** 发布说明（GitHub Release body，可能为 null）。 */
  notes: string | null
  /** 匹配到的安装包资产（无匹配为 null）。 */
  assetName: string | null
  assetUrl: string | null
  assetSize: number | null
  /** true = 便携版（不做应用内更新，仅提示手动下载）。 */
  portable: boolean
}

/** 更新下载进度（壳侧 desktop_update_progress，轮询）。 */
export interface UpdateProgress {
  /** idle / checking / downloading / downloaded / installing / error。 */
  phase: 'idle' | 'checking' | 'downloading' | 'downloaded' | 'installing' | 'error'
  received: number
  total: number
  message: string | null
}

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
  /** 检查 GitHub Releases 是否有新版本。 */
  checkUpdate(): Promise<UpdateInfo>
  /** 下载新版安装包（仅安装版；进度经 getUpdateProgress 轮询）。 */
  downloadUpdate(): Promise<void>
  /** 当前更新下载进度。 */
  getUpdateProgress(): Promise<UpdateProgress>
  /** 安装已下载的新版本（应用会退出并自动重启；仅安装版支持）。 */
  installUpdate(): Promise<void>
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
