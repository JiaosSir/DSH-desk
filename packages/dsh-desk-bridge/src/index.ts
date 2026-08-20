/**
 * @JiaosSir/dsh-desk-bridge —— 宿主半部。
 *
 * 一个经 bundle patch（cordis.patch.yml 把行插入 profile 树）挂进宿主进程的
 * 极简 Cordis 插件。按设计本包不含业务逻辑：宿主半部只旁听宿主事件并提供
 * /api/desktop/* 路由族（阶段 4）；浏览器半部只做特性检测 + 调 Tauri IPC 桥。
 * @module @JiaosSir/dsh-desk-bridge
 */

import type { Context } from '@deepseek-ai/cordis'

/** 稳定的 Cordis 插件名。 */
export const name = 'desk-bridge'

export function apply(_ctx: Context): void {
  // 阶段 4：旁听 approval/request waterfall（永远经 next() 委托——桌面壳
  // 绝不回答审批），并注册 /api/desktop/* 路由（SSE 事件流，供通知镜像）。
}
