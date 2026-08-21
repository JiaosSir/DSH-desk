/**
 * 「桌面」设置区：自启开关、快捷键展示、工作区选择、重启宿主、打开日志、查看最新版。
 * 全部经 DeskBridge 调壳侧 IPC；无桥时整个设置区不注册（等价纯浏览器）。
 * @module @JiaosSir/dsh-desk-bridge/client/settings-panel
 */

import { useEffect, useState } from 'react'
import type { CSSProperties, ReactElement } from 'react'
import type { SettingsSectionOwnerProps } from '@deepseek-ai/dsh-client-ui-settings/client'
import type { DeskBridge } from './bridge'

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
const buttonStyle: CSSProperties = {
  padding: '5px 12px',
  fontSize: 12,
  borderRadius: 6,
  cursor: 'pointer',
  border: '1px solid rgba(128, 128, 128, 0.35)',
  background: 'transparent',
  color: 'inherit',
}

/** 「桌面」设置区（v1 首发 zh-CN 文案）。 */
export function DesktopSettings({ bridge }: DesktopSettingsProps): ReactElement {
  const [autostart, setAutostart] = useState(false)
  const [hotkey, setHotkey] = useState('')
  const [workspace, setWorkspace] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    void bridge.getAutostart().then(setAutostart).catch(() => {})
    void bridge.getHotkey().then(setHotkey).catch(() => {})
    void bridge.getWorkspace().then(setWorkspace).catch(() => {})
  }, [bridge])

  const toggleAutostart = async (): Promise<void> => {
    setBusy(true)
    try {
      setAutostart(await bridge.setAutostart(!autostart))
    } catch {
      // 壳侧失败保持原状态。
    } finally {
      setBusy(false)
    }
  }

  const pickWorkspace = async (): Promise<void> => {
    setBusy(true)
    try {
      const path = await bridge.pickFolder()
      if (path !== null) setWorkspace(path)
    } catch {
      // 取消或失败保持原状态。
    } finally {
      setBusy(false)
    }
  }

  return (
    <div style={{ padding: '4px 0' }}>
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
          <div style={labelStyle}>工作区</div>
          <div style={hintStyle}>{workspace ?? '未选择'}</div>
        </div>
        <button type="button" style={buttonStyle} disabled={busy} onClick={() => void pickWorkspace()}>
          选择…
        </button>
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
          <div style={labelStyle}>日志与更新</div>
        </div>
        <span style={{ display: 'flex', gap: 8 }}>
          <button type="button" style={buttonStyle} onClick={() => void bridge.openLogs()}>
            打开日志
          </button>
          <button type="button" style={buttonStyle} onClick={() => void bridge.openReleases()}>
            查看最新版
          </button>
        </span>
      </div>
    </div>
  )
}
