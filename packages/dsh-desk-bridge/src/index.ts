/**
 * @cjiaojiao/dsh-desk-bridge —— 宿主半部。
 *
 * 经 bundle patch（cordis.patch.yml 把行插入 profile 树）挂进宿主进程的极简
 * Cordis 插件：不含业务逻辑，只旁听宿主事件并提供 /api/desktop/* 路由族。
 * 不变式 3 落实点：审批旁听者永远经 next() 委托——桌面壳绝不回答审批，
 * 只把请求镜像给 SSE 订阅者（通知镜像用）。
 * @module @cjiaojiao/dsh-desk-bridge
 */

import type { Context } from '@deepseek-ai/cordis'
import type { WebRoute } from '@deepseek-ai/dsh-host-webserver'
// 类型面：审批 waterfall 事件（approval/request）与请求形状；运行时零依赖。
import type {} from '@deepseek-ai/dsh-user-approval'
import type { IncomingMessage, ServerResponse } from 'node:http'

/** 稳定的 Cordis 插件名。 */
export const name = 'desk-bridge'

/** 宿主面依赖：webServer 路由注册（dsh-web-app 组合提供）。 */
export const inject = ['webServer']

/** 健康检查路由路径（浏览器半部的最小端到端闭环）。 */
export const HEALTH_PATH = '/api/desktop/health'
/** SSE 事件流路由路径（审批镜像等桌面事件）。 */
export const EVENTS_PATH = '/api/desktop/events'

/** 镜像给浏览器半部的桌面事件（SSE data 帧）。 */
export interface DesktopEvent {
  type: 'approval'
  toolName: string
  reason?: string
}

/** 回环地址集合（SSE 等路由的信任栅栏，与 dsh-ssh 的 loopback 栅栏同构）。 */
const LOOPBACK_ADDRESSES = new Set(['127.0.0.1', '::1', '::ffff:127.0.0.1'])

/** 请求是否来自本机回环。 */
export function isLoopbackRequest(req: IncomingMessage): boolean {
  const remote = req.socket?.remoteAddress
  return remote !== undefined && LOOPBACK_ADDRESSES.has(remote)
}

/** JSON 响应（utf-8 + no-referrer，与 dsh-ssh 同构）。 */
function writeJson(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'referrer-policy': 'no-referrer',
  })
  res.end(JSON.stringify(body))
}

/** 健康路由：GET 返回 { ok: true }，其余 405。 */
export function makeHealthRoute(): WebRoute {
  return {
    kind: 'exact',
    path: HEALTH_PATH,
    handler: (req, res) => {
      if (req.method !== 'GET') {
        writeJson(res, 405, { error: 'method not allowed' })
        return
      }
      writeJson(res, 200, { ok: true })
    },
  }
}

/** 桌面事件总线：宿主内多播（审批旁听 → 所有 SSE 订阅者）。 */
export interface DesktopEventBus {
  publish(event: DesktopEvent): void
  subscribe(listener: (event: DesktopEvent) => void): () => void
}

export function createEventBus(): DesktopEventBus {
  const listeners = new Set<(event: DesktopEvent) => void>()
  return {
    publish(event) {
      for (const listener of [...listeners]) {
        try {
          listener(event)
        } catch {
          // 单个订阅者（含写已断连接）出错不得拖垮旁听链。
        }
      }
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

/** SSE 事件流路由：回环 + GET 栅栏，挂住连接推送审批镜像。 */
export function makeEventsRoute(bus: DesktopEventBus): WebRoute {
  return {
    kind: 'exact',
    path: EVENTS_PATH,
    handler: (req, res) => {
      if (!isLoopbackRequest(req)) {
        writeJson(res, 403, { error: 'forbidden: loopback-only' })
        return
      }
      if (req.method !== 'GET') {
        writeJson(res, 405, { error: 'method not allowed' })
        return
      }
      res.writeHead(200, {
        'content-type': 'text/event-stream; charset=utf-8',
        'cache-control': 'no-cache',
        connection: 'keep-alive',
      })
      res.write('retry: 3000\n\n')
      const unsubscribe = bus.subscribe((event) => {
        try {
          res.write(`data: ${JSON.stringify(event)}\n\n`)
        } catch {
          // 连接已断；close 事件会清理订阅。
        }
      })
      const cleanup = (): void => {
        unsubscribe()
      }
      req.on('close', cleanup)
      res.on('close', cleanup)
    },
  }
}

export function apply(ctx: Context): void {
  const bus = createEventBus()
  ctx.effect(() => ctx.webServer.register(makeHealthRoute()), 'desk-bridge: health route')
  ctx.effect(() => ctx.webServer.register(makeEventsRoute(bus)), 'desk-bridge: events route')
  // 审批旁听（不变式 3）：只镜像、绝不回答——返回 next() 即委托给真正的应答者。
  ctx.on('approval/request', (req, next) => {
    bus.publish({ type: 'approval', toolName: req.toolName, reason: req.reason })
    return next()
  })
}
