/**
 * @cjiaojiao/dsh-desk-bridge —— 浏览器半部。
 *
 * 经 __DSH_BOOT__ 模块图由 Web 壳加载的 Cordis client 插件：有桌面桥时注册
 * 「桌面」设置区（settings.section 槽位）并订阅审批 SSE 做通知镜像；
 * 纯浏览器里退化为空操作，同一份 UI 功能保持完整。
 * @module @cjiaojiao/dsh-desk-bridge/client
 */

import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
// 类型面：settings.section 槽位声明（dsh-client-ui-slots 的 SlotMap 合并）。
import type {} from '@deepseek-ai/dsh-client-ui-slots'
import { detectBridge } from './bridge'
import { subscribeNotifications } from './notifications'
import { DesktopSettingsSection } from './settings-panel'

/** 本插件拥有的 locale 命名空间。 */
const NS = 'dsh-desk-bridge'

/** 所需服务（槽位注册与生命周期）。 */
export const inject = ['slots']

export function apply(ctx: ClientContext): void {
  const bridge = detectBridge()
  if (bridge === null) {
    // 纯浏览器：无桌面能力、无 UI 面、无订阅。
    console.info(`[${NS}] 运行在纯浏览器中——已退化，空操作`)
    return
  }
  // 「桌面」设置区：槽位由 settings 壳声明，inject 等待声明并随其折叠自动卸载。
  ctx.slots.inject('settings.section', () => ctx.slots.register(
    { name: 'settings.section', id: 'desktop', order: 100, label: '桌面' },
    DesktopSettingsSection(bridge),
  ))
  // 审批通知镜像（EventSource 自动重连；dispose 时断开）。
  const stopNotifications = subscribeNotifications(bridge)
  ctx.effect(() => stopNotifications, `${NS}: notifications`)
  console.info(`[${NS}] 检测到桌面桥`)
}
