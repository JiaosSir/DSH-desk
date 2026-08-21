/**
 * 宿主半部的路由与事件契约：health 路由、SSE 事件流、审批旁听。
 * mock ctx 断言 register/on 调用与 handler 的 HTTP 语义，不依赖真实 harness。
 */

import { describe, expect, it, vi } from 'vitest'
import {
  apply,
  createEventBus,
  EVENTS_PATH,
  HEALTH_PATH,
  isLoopbackRequest,
  makeEventsRoute,
  makeHealthRoute,
} from '../src/index'

function mockCtx() {
  const register = vi.fn(() => () => {})
  const listeners: Record<string, (...args: never[]) => unknown> = {}
  const on = vi.fn((event: string, fn: (...args: never[]) => unknown) => {
    listeners[event] = fn
    return () => {}
  })
  return {
    webServer: { register },
    effect: vi.fn((fn: () => () => void) => fn()),
    on,
    _register: register,
    _listeners: listeners,
  } as never
}

function mockRes() {
  return { writeHead: vi.fn(), end: vi.fn(), write: vi.fn(), on: vi.fn() }
}

function mockReq(method: string, remoteAddress: string | undefined) {
  return {
    method,
    socket: { remoteAddress },
    on: vi.fn(),
  }
}

describe('宿主半部', () => {
  it('apply 注册健康路由到 webServer', () => {
    const ctx = mockCtx()
    apply(ctx)
    const anyCtx = ctx as { _register: ReturnType<typeof vi.fn> }
    const paths = anyCtx._register.mock.calls.map((c: unknown[]) => (c[0] as { path: string }).path)
    expect(paths).toContain(HEALTH_PATH)
  })

  it('健康路由 GET 返回 { ok: true }', () => {
    const route = makeHealthRoute()
    const res = mockRes()
    route.handler(mockReq('GET', '127.0.0.1') as never, res as never)
    expect(res.writeHead).toHaveBeenCalledWith(200, expect.anything())
    const body = JSON.parse(String(res.end.mock.calls[0][0]))
    expect(body.ok).toBe(true)
  })

  it('健康路由非 GET 返回 405', () => {
    const route = makeHealthRoute()
    const res = mockRes()
    route.handler(mockReq('POST', '127.0.0.1') as never, res as never)
    expect(res.writeHead).toHaveBeenCalledWith(405, expect.anything())
  })

  it('回环判定：本机地址放行、局域网拒绝', () => {
    expect(isLoopbackRequest(mockReq('GET', '127.0.0.1') as never)).toBe(true)
    expect(isLoopbackRequest(mockReq('GET', '::1') as never)).toBe(true)
    expect(isLoopbackRequest(mockReq('GET', '192.168.1.5') as never)).toBe(false)
  })

  it('事件总线：发布到达订阅者，退订后不再收到', () => {
    const bus = createEventBus()
    const seen: unknown[] = []
    const off = bus.subscribe((event) => { seen.push(event) })
    bus.publish({ type: 'approval', toolName: 'bash', reason: '写文件' })
    expect(seen).toHaveLength(1)
    off()
    bus.publish({ type: 'approval', toolName: 'write', reason: '写文件' })
    expect(seen).toHaveLength(1)
  })

  it('SSE 路由：GET 挂流、非回环 403、非 GET 405', () => {
    const bus = createEventBus()
    const route = makeEventsRoute(bus)
    const res = mockRes()
    route.handler(mockReq('GET', '127.0.0.1') as never, res as never)
    expect(res.writeHead).toHaveBeenCalledWith(
      200,
      expect.objectContaining({ 'content-type': 'text/event-stream; charset=utf-8' }),
    )
    expect(res.write).toHaveBeenCalled()

    const denied = mockRes()
    route.handler(mockReq('GET', '192.168.1.5') as never, denied as never)
    expect(denied.writeHead).toHaveBeenCalledWith(403, expect.anything())

    const badMethod = mockRes()
    route.handler(mockReq('POST', '127.0.0.1') as never, badMethod as never)
    expect(badMethod.writeHead).toHaveBeenCalledWith(405, expect.anything())
  })

  it('审批旁听：镜像到 SSE 订阅者且永远委托 next()', async () => {
    const ctx = mockCtx()
    apply(ctx)
    const anyCtx = ctx as {
      _register: ReturnType<typeof vi.fn>
      _listeners: Record<string, (...args: never[]) => unknown>
    }
    const routes = anyCtx._register.mock.calls.map((c: unknown[]) => (
      c[0] as { path: string; handler: (req: unknown, res: unknown) => void }
    ))
    const eventsRoute = routes.find((r) => r.path === EVENTS_PATH)
    expect(eventsRoute).toBeDefined()

    const res = mockRes()
    eventsRoute!.handler(mockReq('GET', '127.0.0.1') as never, res as never)

    const listener = anyCtx._listeners['approval/request']
    expect(listener).toBeDefined()
    const next = vi.fn(async () => 'allowed-once')
    const outcome = await listener(
      { id: 'a1', toolName: 'bash', reason: '写文件' } as never,
      next as never,
    )
    expect(next).toHaveBeenCalledOnce()
    expect(outcome).toBe('allowed-once')

    const written = (res.write as ReturnType<typeof vi.fn>).mock.calls
      .map((c: unknown[]) => String(c[0]))
      .join('')
    expect(written).toContain('data: {"type":"approval"')
    expect(written).toContain('bash')
  })
})
