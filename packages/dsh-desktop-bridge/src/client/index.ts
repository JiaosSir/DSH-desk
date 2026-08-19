/**
 * @JiaosSir/dsh-desktop-bridge —— 浏览器半部。
 *
 * 一个经 __DSH_BOOT__ 模块图（见 package.json 的 `dsh.client` 声明）由 Web
 * 壳加载的 Cordis client 插件。它只做特性检测：有桌面桥时挂"桌面"设置区与
 * 通知镜像（阶段 4）；纯浏览器里退化为空操作，同一份 UI 功能保持完整。
 * @module @JiaosSir/dsh-desktop-bridge/client
 */

import type { Context } from '@deepseek-ai/cordis'
import { detectBridge } from './bridge'

/** 本插件拥有的 locale 命名空间。 */
const NS = 'dsh-desktop-bridge'

/** 所需服务（阶段 4：slots、locale、settings）。 */
export const inject = [] as const

export function apply(ctx: Context): void {
  const bridge = detectBridge()
  if (bridge === null) {
    // 纯浏览器：无桌面能力、无 UI 面、无订阅。
    console.info(`[${NS}] 运行在纯浏览器中——已退化，空操作`)
    return
  }
  // 阶段 4：在此注册设置区与审批事件订阅（通知镜像）。
  console.info(`[${NS}] 检测到桌面桥`)
}
