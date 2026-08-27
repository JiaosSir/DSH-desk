/**
 * 「桌面」设置区：自启开关、快捷键展示、工作区选择、重启宿主、打开日志、检查更新。
 * 检查更新：安装版支持应用内下载并静默更新（自动退出/覆盖安装/重启）；
 * 便携版不做应用内更新，提示前往 GitHub Releases 手动下载覆盖。
 * 全部经 DeskBridge 调壳侧 IPC；无桥时整个设置区不注册（等价纯浏览器）。
 * @module @cjiaojiao/dsh-desk-bridge/client/settings-panel
 */

import { useEffect, useState } from 'react'
import type { CSSProperties, ReactElement } from 'react'
import type { SettingsSectionOwnerProps } from '@deepseek-ai/dsh-client-ui-settings/client'
import type { DeskBridge, UpdateInfo, UpdateProgress } from './bridge'

export interface DesktopSettingsProps {
  bridge: DeskBridge
  /** 设置壳提供的关闭入口（离开设置面的流程用）。 */
  close: () => void
}

/** 槽位注册用的组件工厂：把桥绑进 props，把壳的 close 透传（index.ts 无 JSX）。 */
export function DesktopSettingsSection(bridge: DeskBridge) {
  return function Section(props: SettingsSectionOwnerProps): ReactElement {
    return <DesktopSettings bridge={bridge} close={props.close} />
  }
}

const rowStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 12,
  padding: '10px 0',
  borderBottom: '1px solid rgba(128, 128, 128, 0.15)',
}
const labelStyle: CSSProperties = { fontSize: 13 }
const hintStyle: CSSProperties = { fontSize: 12, opacity: 0.6, marginTop: 2 }
const errorStyle: CSSProperties = { fontSize: 12, color: '#ff8f98', marginTop: 4 }
const buttonStyle: CSSProperties = {
  padding: '5px 12px',
  fontSize: 12,
  borderRadius: 6,
  cursor: 'pointer',
  border: '1px solid rgba(128, 128, 128, 0.35)',
  background: 'transparent',
  color: 'inherit',
}
const buttonPrimaryStyle: CSSProperties = {
  ...buttonStyle,
  background: 'rgba(80, 130, 255, 0.15)',
  borderColor: 'rgba(80, 130, 255, 0.6)',
}

/** 字节数 → 人类可读大小。 */
function formatSize(bytes: number): string {
  if (bytes <= 0) return ''
  const mb = bytes / 1024 / 1024
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(1)} MB`
}

/** 发布说明摘录（Markdown 按纯文本展示，截断）。 */
function notesExcerpt(notes: string): string {
  const text = notes.replace(/[#>*`\[\]_-]+/g, ' ').replace(/\s+/g, ' ').trim()
  return text.length > 200 ? `${text.slice(0, 200)}…` : text
}

/** 「桌面」设置区（v1 首发 zh-CN 文案）。 */
export function DesktopSettings({ bridge }: DesktopSettingsProps): ReactElement {
  const [autostart, setAutostart] = useState(false)
  const [hotkey, setHotkey] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 检查更新状态机：updateInfo = 检查结果；download 阶段 idle/downloading/downloaded/error。
  const [checking, setChecking] = useState(false)
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null)
  const [downloadPhase, setDownloadPhase] = useState<'idle' | 'downloading' | 'downloaded' | 'error'>('idle')
  const [progress, setProgress] = useState<UpdateProgress | null>(null)

  useEffect(() => {
    void bridge.getAutostart().then(setAutostart).catch(() => {})
    void bridge.getHotkey().then(setHotkey).catch(() => {})
  }, [bridge])

  // 下载期间轮询壳侧进度（复用等待页轮询模式；300ms 一拍）。
  useEffect(() => {
    if (downloadPhase !== 'downloading') return
    const timer = window.setInterval(() => {
      void bridge
        .getUpdateProgress()
        .then((p) => {
          setProgress(p)
          if (p.phase === 'downloaded') setDownloadPhase('downloaded')
          if (p.phase === 'error') setDownloadPhase('error')
        })
        .catch(() => {})
    }, 300)
    return () => window.clearInterval(timer)
  }, [downloadPhase, bridge])

  /** 统一失败反馈：按钮不能静默死掉。 */
  const run = async (action: () => Promise<unknown>): Promise<void> => {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const toggleAutostart = async (): Promise<void> => {
    await run(async () => {
      setAutostart(await bridge.setAutostart(!autostart))
    })
  }

  const checkUpdate = async (): Promise<void> => {
    await run(async () => {
      setChecking(true)
      setUpdateInfo(null)
      setDownloadPhase('idle')
      setProgress(null)
      try {
        setUpdateInfo(await bridge.checkUpdate())
      } finally {
        setChecking(false)
      }
    })
  }

  const downloadUpdate = async (): Promise<void> => {
    await run(async () => {
      setDownloadPhase('downloading')
      setProgress(null)
      await bridge.downloadUpdate()
      // 命令在下载完成后才返回；补一拍确认终态。
      const p = await bridge.getUpdateProgress()
      setProgress(p)
      setDownloadPhase(p.phase === 'downloaded' ? 'downloaded' : 'error')
    })
  }

  return (
    <div style={{ padding: '4px 0' }}>
      {error !== null && (
        <div style={{ fontSize: 12, color: '#ff8f98', padding: '6px 0' }}>操作失败：{error}</div>
      )}
      <div style={rowStyle}>
        <div>
          <div style={labelStyle}>开机自启</div>
          <div style={hintStyle}>登录 Windows 后自动启动 DSH-desk</div>
        </div>
        <label>
          <input
            type="checkbox"
            checked={autostart}
            disabled={busy}
            onChange={() => void toggleAutostart()}
          />
        </label>
      </div>
      <div style={rowStyle}>
        <div>
          <div style={labelStyle}>全局快捷键</div>
          <div style={hintStyle}>{hotkey || '…'}（唤起/隐藏窗口）</div>
        </div>
      </div>
      <div style={rowStyle}>
        <div>
          <div style={labelStyle}>宿主</div>
          <div style={hintStyle}>新装插件在宿主重启后生效</div>
        </div>
        <button type="button" style={buttonStyle} onClick={() => void bridge.restartHost()}>
          重启宿主
        </button>
      </div>
      <div style={rowStyle}>
        <div>
          <div style={labelStyle}>日志</div>
        </div>
        <button type="button" style={buttonStyle} onClick={() => void bridge.openLogs()}>
          打开日志
        </button>
      </div>
      <div style={rowStyle}>
        <div>
          <div style={labelStyle}>检查更新</div>
          <div style={hintStyle}>从 GitHub Releases 检测最新版本</div>
        </div>
        <button
          type="button"
          style={buttonStyle}
          disabled={busy || checking}
          onClick={() => void checkUpdate()}
        >
          {checking ? '检查中…' : '检查更新'}
        </button>
      </div>
      {updateInfo !== null && (
        <div style={{ padding: '10px 0', borderBottom: '1px solid rgba(128, 128, 128, 0.15)' }}>
          {!updateInfo.available ? (
            <div style={hintStyle}>已是最新版本 {updateInfo.latestVersion}</div>
          ) : updateInfo.portable ? (
            <div>
              <div style={labelStyle}>
                发现新版本 {updateInfo.latestVersion}（当前 {updateInfo.currentVersion}）
              </div>
              <div style={hintStyle}>
                便携版不做应用内更新：请前往 GitHub Releases 下载最新压缩包，解压后覆盖到当前目录。
              </div>
              <div style={{ marginTop: 8 }}>
                <button type="button" style={buttonPrimaryStyle} onClick={() => void bridge.openReleases()}>
                  前往 Releases 下载
                </button>
              </div>
            </div>
          ) : (
            <div>
              <div style={labelStyle}>
                发现新版本 {updateInfo.latestVersion}（当前 {updateInfo.currentVersion}）
              </div>
              {updateInfo.notes !== null && updateInfo.notes.trim() !== '' && (
                <div style={hintStyle}>{notesExcerpt(updateInfo.notes)}</div>
              )}
              {downloadPhase === 'idle' && (
                <div style={{ marginTop: 8 }}>
                  <button
                    type="button"
                    style={buttonPrimaryStyle}
                    disabled={busy || updateInfo.assetUrl === null}
                    onClick={() => void downloadUpdate()}
                  >
                    下载并更新{updateInfo.assetSize !== null ? `（${formatSize(updateInfo.assetSize)}）` : ''}
                  </button>
                </div>
              )}
              {downloadPhase === 'downloading' && (
                <div style={{ marginTop: 8 }}>
                  <div style={hintStyle}>
                    正在下载{progress !== null && progress.total > 0
                      ? ` ${((progress.received / progress.total) * 100).toFixed(0)}%`
                      : progress !== null && progress.received > 0
                        ? ` ${formatSize(progress.received)}`
                        : '…'}
                  </div>
                  {progress !== null && progress.total > 0 && (
                    <div
                      style={{
                        marginTop: 6,
                        height: 4,
                        borderRadius: 2,
                        background: 'rgba(128, 128, 128, 0.25)',
                        overflow: 'hidden',
                      }}
                    >
                      <div
                        style={{
                          height: '100%',
                          width: `${Math.min(100, (progress.received / progress.total) * 100).toFixed(1)}%`,
                          background: 'rgba(80, 130, 255, 0.8)',
                          transition: 'width 0.2s',
                        }}
                      />
                    </div>
                  )}
                </div>
              )}
              {downloadPhase === 'downloaded' && (
                <div style={{ marginTop: 8 }}>
                  <div style={hintStyle}>下载完成，安装时应用会退出并自动重启。</div>
                  <button
                    type="button"
                    style={buttonPrimaryStyle}
                    onClick={() => void bridge.installUpdate().catch(() => {})}
                  >
                    立即安装
                  </button>
                </div>
              )}
              {downloadPhase === 'error' && (
                <div style={errorStyle}>{progress?.message ?? '下载失败，请重试'}</div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
