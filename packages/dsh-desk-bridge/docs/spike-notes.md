# Phase 4 Spike Notes：dsh-desk-bridge 的宿主/浏览器半部 API（2026-08-20）

研究来源：`~/.dsh/profiles/web/node_modules/@linxin666/dsh-ssh@0.1.20`（含 src/，rc.6 时代
的同类外部插件）、`@deepseek-ai/dsh-*@0.1.0-rc.8` 的类型声明（bridge 的 devDeps，位于
`packages/dsh-desk-bridge/node_modules`）、上游仓库 `deepseek-harness` 的
`packages/interaction/user-approval` 与 `packages/client/*`。

## 1. 宿主半部（host，exports "."）

### 1.1 路由注册 — `@deepseek-ai/dsh-host-webserver`

`ctx.webServer.register(route)`，`route: WebRoute = { kind: 'exact'|'prefix', path, handler }`。
handler 全权拥有响应生命周期（**可以挂住连接，SSE 官方支持**）。重复 (kind, path) 会抛错。
另有 `registerUpgrade({ path, handler })`（WebSocket）、`registerFallback`、`tapIndex`。
`ctx.webServer.port` / `.host` 可读。插件 `inject: ['webServer']`。

```ts
const dispose = ctx.webServer.register({
  kind: 'exact',
  path: '/api/desktop/health',
  handler: (req, res) => { res.writeHead(200, { 'content-type': 'application/json; charset=utf-8' }); res.end('{"ok":true}') },
})
// 用 ctx.effect(() => dispose, 'label') 挂生命周期
```

### 1.2 审批旁听 — `approval/request` waterfall（不变式 3 的落实点）

事件签名（`@deepseek-ai/dsh-user-approval` 声明）：

```ts
'approval/request'(this: Scoped<ApprovalService>, req: ApprovalRequest, next: () => Promise<ApprovalOutcome>): Promise<ApprovalOutcome>
```

`ApprovalRequest` 含 `id`（与 `approval/asked`/`approval/decided` 会话审计事件同 id 配对）、
`toolName`、`callId?`、`reason?`、`agent`、`signal`。**旁听者永远 `return next()`**——
返回结果即「替用户做决定」，会破坏信任模型；只镜像、绝不回答。事件按 agent scope 过滤。

### 1.3 设置区（schema 驱动，自动渲染进 Web 设置面）— `@deepseek-ai/dsh-settings`

```ts
installSettingsSection(ctx, settingsNamespace('dsh-desk-bridge'), Config /*schemastery*/, entry ?? {}, {
  setSource: (current) => {...},   // 附接设置服务时指向 resolved scope，否则 entry
  onChange: () => {...},           // 附接/提交后重判派生面
})
```

`schemastery` schema 会自动在 Web 设置 UI 渲染表单（dsh-ssh 的 SSH 设置即此法）。
`ctx.settings.register/watch/update/replace/mutate` 是更底层的直接面。

### 1.4 其它宿主面

- `ctx.tools.register(tool)`（schemastery 工具）——给 agent 暴露桌面能力用（后续）。
- `ctx.systemPrompt.section({ name, order, text })` —— 向 agent 公告插件（dsh-ssh 做法，
  建议同样做一条「本机已安装 dsh-desk-bridge」公告）。
- 生命周期一律 `ctx.effect(() => disposer, 'label')`；apply 可被 mount-once 包裹
  （dsh-ssh 有 `mountOnce(id, impl)` 防止重复挂载）。

### 1.5 回环栅栏（安全）

dsh-ssh 每个路由先查 `isLoopbackRequest(req)`（`req.socket.remoteAddress` ∈ 回环集），
非回环回 403。我们的 `/api/desktop/*` 路由照抄该模式（只读 health 可放宽，SSE 与
后续写路由必须保留）。

## 2. 浏览器半部（client，exports "./client"）

### 2.1 设置区注册 — settings 壳有正式槽位（rc.8 新能力）

`dsh-client-ui-settings` 的 SlotMap 声明了 `'settings.section'`（kind: 'list'，owner props
`{ close }`）。外部插件**不需要** dsh-ssh 那套 DOM 注入——直接：

```ts
export const inject = ['slots']
ctx.slots.inject('settings.section', () => ctx.slots.register(
  { name: 'settings.section', id: 'desktop', order: 100, label: '桌面' },
  (props: SettingsSectionOwnerProps) => <DesktopSettings close={props.close} />,
))
```

要点：必须用 `ctx.slots.inject(key, cb)` 包裹（槽位由 settings 壳声明，未声明时注册会
抛错；inject 等待声明、随声明折叠自动卸载）。`label` 为注册方本地化文案（v1 首发 zh-CN，
直接写死中文；后续接 `ctx.locale.register` + `locale: NS` 选项，参照 dsh-ssh）。
需要新增 devDep `@deepseek-ai/dsh-client-ui-slots`（类型面；已在 tsdown 的
PLATFORM_MODULES external 名单里）。

### 2.2 连接面 — `@deepseek-ai/dsh-client-connection`

`ctx.connection.api`（IApiClient：SessionsApi/EventsApi/SettingsApi…）与
`ctx.connection.start(sinks)`（帧流）。但我们走更简单的路：**同源 fetch 自己的
`/api/desktop/*` 路由**（dsh-ssh 的 client 即自建 api.ts + fetch），不依赖 mux 帧。

### 2.3 退化契约

纯浏览器（`detectBridge() === null`）：不注册设置区、不建 SSE、零副作用。
壳内：注册「桌面」设置区（自启开关、快捷键只读展示、打开日志、重启宿主、查看最新版、
web→desktop 插件同步、工作区路径选择）。

## 3. 桥注入协议（壳侧，已在阶段 2/3 就位）

`window.__DSH_DESK__`（壳 initialization_script 注入）→ `packages/dsh-desk-bridge/src/client/bridge.ts`
的 `DeskBridge` 接口逐字段对齐（pickFolder/openLogs/openReleases/restartHost/
setAutostart/getAutostart/quit/notify）。阶段 4 壳侧需补齐命令：`desktop_set_autostart`、
`desktop_get_autostart`、`desktop_notify`、`desktop_open_releases`、`desktop_sync_list`、
`desktop_sync_add`、`desktop_restart_host`、`desktop_get_onboarding`（已完成）。

## 4. 待实现清单（本阶段）

- [x] spike-notes（本文档）
- [ ] 宿主半部：`GET /api/desktop/health` 最小闭环 + 单测（mock ctx 断言 register 调用）
- [ ] 宿主半部：审批旁听（永远 next()）+ `GET /api/desktop/events` SSE + 单测
- [ ] 浏览器半部：设置区（settings.section 槽位）+ 通知镜像（SSE 订阅）+ path-picker 桥
  + 退化测试矩阵（mock `__ModuleLoader__`）
- [ ] 壳侧：autostart/notify/releases/sync/restart_host 命令、Ctrl+Alt+D 快捷键、
  WebView 硬化（禁外部导航）
- [ ] 开发闭环：`dsh plugin --profile desktop add link:<bridge 绝对路径>` + 重启宿主验证
