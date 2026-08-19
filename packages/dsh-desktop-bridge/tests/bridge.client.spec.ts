/**
 * 浏览器半部的退化契约：没有注入桌面桥时插件必须是静默空操作（等价纯浏览器
 * 行为）；有桥时保持可用且暂不触碰任何桥方法（阶段 4 才挂 UI）。
 */

import { describe, expect, it, vi } from 'vitest'
import { apply } from '../src/client/index'
import { detectBridge } from '../src/client/bridge'
import type { DesktopBridge } from '../src/client/bridge'

/** client 根上下文的最小 mock（阶段 1 尚不需要任何服务）。 */
function mockCtx() {
  return {
    effect: vi.fn(),
    get: vi.fn(() => undefined),
    on: vi.fn(),
    provide: vi.fn(),
    inject: vi.fn(),
  } as never
}

function mockBridge(): DesktopBridge {
  return {
    available: true,
    pickFolder: vi.fn(async () => null),
    openLogs: vi.fn(async () => {}),
    openReleases: vi.fn(async () => {}),
    restartHost: vi.fn(async () => {}),
    setAutostart: vi.fn(async () => true),
    getAutostart: vi.fn(async () => false),
    quit: vi.fn(async () => {}),
    notify: vi.fn(async () => {}),
  }
}

describe('桥能力检测', () => {
  it('纯浏览器（无 __DSH_DESKTOP__）退化为 null', () => {
    delete window.__DSH_DESKTOP__
    expect(detectBridge()).toBeNull()
  })

  it('有桥时原样返回注入对象', () => {
    const bridge = mockBridge()
    window.__DSH_DESKTOP__ = bridge
    expect(detectBridge()).toBe(bridge)
    delete window.__DSH_DESKTOP__
  })
})

describe('client 插件入口', () => {
  it('无桥时是静默空操作', () => {
    delete window.__DSH_DESKTOP__
    const info = vi.spyOn(console, 'info').mockImplementation(() => {})
    expect(() => apply(mockCtx())).not.toThrow()
    expect(info).toHaveBeenCalledWith(expect.stringContaining('已退化'))
    info.mockRestore()
  })

  it('有桥时暂不触碰任何桥方法', () => {
    const bridge = mockBridge()
    window.__DSH_DESKTOP__ = bridge
    expect(() => apply(mockCtx())).not.toThrow()
    expect(bridge.notify).not.toHaveBeenCalled()
    delete window.__DSH_DESKTOP__
  })
})
