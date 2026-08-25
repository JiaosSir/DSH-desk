/**
 * 审批通知镜像：订阅宿主 /api/desktop/events SSE，收到审批事件经桥弹系统通知。
 * 纯浏览器（无桥）由调用方保证不启动；SSE 断线由浏览器 EventSource 自动重连。
 * @module @cjiaojiao/dsh-desk-bridge/client/notifications
 */

import type { DeskBridge } from './bridge'

/** 宿主推送的桌面事件（与宿主半部 DesktopEvent 同构）。 */
export interface ApprovalEvent {
  type: 'approval'
  toolName: string
  reason?: string
}

/**
 * 订阅审批镜像并转发为系统通知。
 * @param bridge - 注入的桌面桥。
 * @returns 停止并断开订阅的 disposer。
 */
export function subscribeNotifications(bridge: DeskBridge): () => void {
  if (typeof EventSource === 'undefined') return () => {}
  const source = new EventSource('/api/desktop/events')
  source.onmessage = (message) => {
    try {
      const event = JSON.parse(message.data as string) as ApprovalEvent
      if (event.type === 'approval') {
        const body = event.reason !== undefined ? `${event.toolName}：${event.reason}` : event.toolName
        void bridge.notify('审批请求', body).catch(() => {})
      }
    } catch {
      // 非 JSON 或未知帧：忽略。
    }
  }
  return () => {
    source.close()
  }
}
