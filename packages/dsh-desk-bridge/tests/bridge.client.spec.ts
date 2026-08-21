/**
 * 浏览器半部的退化契约与有桥行为：
 * - 没有注入桌面桥时插件必须是静默空操作（等价纯浏览器行为）；
 * - 有桥时注册「桌面」设置区、订阅审批 SSE 镜像通知，且不主动触碰桥的写方法。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { apply } from '../src/client/index'
import { detectBridge } from '../src/client/bridge'

/** client 根上下文的最小 mock（阶段 4 需要 slots 与 effect）。 */
interface MockCtx {
  slots: {
    inject: ReturnType<typeof vi.fn>
    register: ReturnType<typeof vi.fn>
  }
  effect: ReturnType<typeof vi.fn>
  get: ReturnType<typeof vi.fn>
  on: ReturnType<typeof vi.fn>
  provide: ReturnType<typeof vi.fn>
  inject: ReturnType<typeof vi.fn>
}

function mockCtx(): MockCtx {
  return {
    slots: {
      inject: vi.fn((_name: string, cb: () => unknown) => cb()),
      register: vi.fn(() => () => {}),
    },
    effect: vi.fn(),
    get: vi.fn(() => undefined),
    on: vi.fn(),
    provide: vi.fn(),
    inject: vi.fn(),
  }
}

function mockBridge() {
  return {
    available: true,
    pickFolder: vi.fn(async () => null),
    getWorkspace: vi.fn(async () => null),
    getHotkey: vi.fn(async () => 'Ctrl+Alt+D'),
    openLogs: vi.fn(async () => {}),
    openReleases: vi.fn(async () => {}),
    restartHost: vi.fn(async () => {}),
    setAutostart: vi.fn(async () => true),
    getAutostart: vi.fn(async () => false),
    quit: vi.fn(async () => {}),
    notify: vi.fn(async () => {}),
  }
}

/** jsdom 没有 EventSource：记录实例的极简桩。 */
class MockEventSource {
  static instances: MockEventSource[] = []
  onmessage: ((message: { data: string }) => void) | null = null
  closed = false
  constructor(public readonly url: string) {
    MockEventSource.instances.push(this)
  }
  close(): void {
    this.closed = true
  }
}

beforeEach(() => {
  MockEventSource.instances = []
  vi.stubGlobal('EventSource', MockEventSource)
})

afterEach(() => {
  vi.unstubAllGlobals()
  delete window.__DSH_DESK__
})

describe('桥能力检测', () => {
  it('纯浏览器（无 __DSH_DESK__）退化为 null', () => {
    delete window.__DSH_DESK__
    expect(detectBridge()).toBeNull()
  })

  it('有桥时原样返回注入对象', () => {
    const bridge = mockBridge()
    window.__DSH_DESK__ = bridge
    expect(detectBridge()).toBe(bridge)
    delete window.__DSH_DESK__
  })
})

describe('client 插件入口', () => {
  it('无桥时是静默空操作：不注册槽位、不建 SSE', () => {
    delete window.__DSH_DESK__
    const ctx = mockCtx()
    const info = vi.spyOn(console, 'info').mockImplementation(() => {})
    expect(() => apply(ctx as never)).not.toThrow()
    expect(ctx.slots.inject).not.toHaveBeenCalled()
    expect(MockEventSource.instances).toHaveLength(0)
    expect(info).toHaveBeenCalledWith(expect.stringContaining('已退化'))
    info.mockRestore()
  })

  it('有桥时注册「桌面」设置区（settings.section 槽位）', () => {
    window.__DSH_DESK__ = mockBridge()
    const ctx = mockCtx()
    apply(ctx as never)
    expect(ctx.slots.inject).toHaveBeenCalledOnce()
    expect(ctx.slots.inject.mock.calls[0][0]).toBe('settings.section')
    expect(ctx.slots.register).toHaveBeenCalledOnce()
    const options = ctx.slots.register.mock.calls[0][0]
    expect(options.name).toBe('settings.section')
    expect(options.id).toBe('desktop')
    expect(options.label).toBe('桌面')
  })

  it('有桥时订阅审批 SSE 并把审批帧镜像为系统通知', () => {
    const bridge = mockBridge()
    window.__DSH_DESK__ = bridge
    const ctx = mockCtx()
    apply(ctx as never)
    expect(MockEventSource.instances).toHaveLength(1)
    const source = MockEventSource.instances[0]
    expect(source.url).toBe('/api/desktop/events')
    expect(source.onmessage).not.toBeNull()
    source.onmessage!({ data: JSON.stringify({ type: 'approval', toolName: 'bash', reason: '写文件' }) })
    expect(bridge.notify).toHaveBeenCalledOnce()
    expect(bridge.notify).toHaveBeenCalledWith('审批请求', 'bash：写文件')
  })

  it('非审批帧不触发通知', () => {
    const bridge = mockBridge()
    window.__DSH_DESK__ = bridge
    const ctx = mockCtx()
    apply(ctx as never)
    const source = MockEventSource.instances[0]
    source.onmessage!({ data: JSON.stringify({ type: 'other' }) })
    source.onmessage!({ data: 'not-json' })
    expect(bridge.notify).not.toHaveBeenCalled()
  })

  it('有桥时暂不主动触碰桥的写方法', () => {
    const bridge = mockBridge()
    window.__DSH_DESK__ = bridge
    const ctx = mockCtx()
    apply(ctx as never)
    expect(bridge.setAutostart).not.toHaveBeenCalled()
    expect(bridge.notify).not.toHaveBeenCalled()
    expect(bridge.restartHost).not.toHaveBeenCalled()
  })
})
