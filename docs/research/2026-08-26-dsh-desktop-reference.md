# 参考项目调研：dsh-desktop 的技术架构与「如何组装 harness」

> 调研对象：[anywhere-labs/dsh-desktop](https://github.com/anywhere-labs/dsh-desktop)（`examples/dsh-desktop-master/` 本地副本）。
> 调研日期：2026-08-26。结论经三路独立核查（主会话 + 两个子代理），所有引用均带文件行号。
> 关联文档：[`docs/plans/2026-08-26-plugin-first-architecture.md`](../plans/2026-08-26-plugin-first-architecture.md)（基于本文的转型方案）。

## 一句话结论

**dsh-desktop 的组装哲学是「壳即插件」**：桌面壳以 Cordis 插件身份**长进 harness 的插件树**（Electron main 进程内直接调用上游 `boot()`），上游保持原样、改动全部以 **yarn 补丁层**承载。我们的 DSH-desk 是「壳即外挂」：Tauri 壳在进程外监督 sidecar 子进程。两条路线取舍不同，但「web-app 只登记 bundles、绝不 pnpm add」是两边的共识。

## 一、项目定位

- Electron 桌面壳（Windows/macOS），把 DeepSeek Harness 0.1.1-rc.2 的 Web UI、Host 服务与插件系统集成进原生应用；
- **万物皆插件，桌面本身也是插件**：桌面壳（窗口/托盘/终端/更新/工作配置）是名为 `dsh-plugin-desktop` 的普通 DSH 插件，与官方、第三方插件走同一条组合路径（README.md:28、:129）；
- 上游处理三件套：`upstream.json`（钉死源码 commit `b150a551`）+ `deepseek-harness/` 只读子模块 + 根 package.json `resolutions` 里的 **17 个 yarn 补丁**（`.yarnrc.yml` 无 patch 配置，补丁声明在 resolutions 的 `patch:` 协议，见第三节）。

## 二、如何组装 harness（六步）

### 第 1 步：装料——上游怎么进来

| 组件 | 作用 | 证据 |
|---|---|---|
| `upstream.json` | 钉死源码 commit 与版本（`sourceVersion` / `runtimePackageVersion` 双记录） | upstream.json:1-6 |
| `deepseek-harness/` 子模块 | 上游完整源码，**只读参考**；`git submodule update --init` 后才有内容 | AGENTS.md；笔记 2026-08-15-pinned-upstream-and-isolated-yarn-workspace.md |
| `patches/` + `resolutions` | Yarn 4 `patch:` 协议给 npm 包打补丁，安装时物化进 node_modules | package.json:16-50；yarn.lock 补丁定位符带 `&hash=` |

### 第 2 步：点火——启动链

```
yarn dev / yarn start
  └─ node lib/bin.js（src/bin.ts）     ← 纯点火器，只 spawn Electron
       └─ spawn Electron → lib/main.js（src/main.ts）  ← 全部装配在这里
```

证据：bin.ts:69-101（`launchElectron()`）、根 package.json:88-97（dev/start/upstream:* 脚本）。

### 第 3 步：配料——profile 组装（`prepareDesktopProfile`）

产出 `boot()` 三要素（profile.ts:724-1043）：

1. **rootConfig** = `<profileDir>/cordis.yml`，boot 前覆写为 `[]`（**空根**，内容全来自 patches）（:755-757）；
2. **bareModuleBaseUrl** = profile 的 `package.json` 的 file:// URL，Loader 解析裸包名的基准（:756）；
3. **patches** 按序堆叠：bundle 层（dsh-base、dsh-web-app）→ **桌面插件层**（web-app 之后插入）→ profile 用户层 → `~/.dsh` 机器层（:826-831），`composeEntries` 校验 id 唯一（:832-834）。

profile 初始化（`ensureDesktopProfile`，:303-327）直接用 **`PROFILE_TEMPLATES.web` 官方 web 模板**初始化 bundles（安装层前置、第三方保序、废弃包名有专门清理集合），**web-app 从不 pnpm add**——这正是我们在 2026-08-26 治本修复中对齐的做法。

### 第 4 步：下锅——同进程 boot()（最关键）

Electron main 进程内直接调用上游 `boot()`（main.ts:811-944）：

```ts
const ctx = await boot(
  'dsh-plugin-desktop',      // bin 名
  prepared.rootConfig,       // cordis.yml（空根）
  prepared.patches,          // 组合后的全部补丁层
  async (hostCtx) => { ... },// prepare 回调：Loader 装好后、树挂载前
  prepared.bareModuleBaseUrl // 裸包名解析基准
)
```

`boot()` = 建 Cordis Context → 装 Loader → 跑 prepare 回调 → 挂 `cordis:include` → **等整棵插件树 settle 才返回**（dsh-app-boot d.ts:220-249）。桌面代码以插件行（`desktop-shell` 等 7 个）长在树里，与官方/第三方插件无特权差异。

### 第 5 步：调味——桌面能力注入

- **patch 层**：`cordis.patch.yml` 插入 `desktop-shell` / `desktop-terminal` / `desktop-diagnostics` / `desktop-notifications` / `desktop-pnpm` / `desktop-profiles` / `desktop-updates`，并改写 `web-runtime`（openBrowser:false）；
- **prepare 回调**：`hostCtx.provide('desktopRuntime', runtime)` 注入 Electron 适配器、`hostCtx.plugin(DesktopProfileService)` 注册公开服务、`provideCmdline` 模拟 `--port`（main.ts:816-943）。

### 第 6 步：出锅——生命周期与健康门禁

- 每次启动 = 一个 `DesktopStartupGeneration`（随机 UUID，拥有 Host + 进程内资源，释放幂等）（startup-generation.ts:30-112）；
- 渲染进程完全沙箱化（`contextIsolation + sandbox`，无 nodeIntegration），URL 带桌面信息查询参数（mode/platform/version/material）；
- 页面加载后回 POST 渲染 boot 报告，主进程判定 healthy 才提交 checkpoint（main.ts:958-975）。

### 补：模块解析 hook（为什么他们不踩「双副本」坑）

`src/module-resolution.ts:47-141`：挂钩 Node CJS `_resolveFilename` + ESM `registerHooks`，**只对 `@deepseek-ai/cordis-plugin-loader` 发出的裸包名 import 生效**，解析策略「安装副本优先 + profile 回退」——显式统一解析，从机制上杜绝同包双副本（我们 2026-08-26 的 Symbol 分裂事故在他们那里不可能发生）。

## 三、补丁机制（vendor 模式）

### 机制

- 声明在根 package.json `resolutions`（**不在 .yarnrc.yml**），`patch:` 协议，精确版 + `^` 范围双条目映射同一补丁文件（package.json:18-19）；
- yarn.lock 里每个被补丁包是独立定位符（`::locator=deepseek-harness-desktop%40workspace%3A.` + `&hash=`），安装时把补丁后的完整内容物化进 `node_modules/@deepseek-ai/*`；
- 补丁全部打在 **npm 发布包的构建产物**（`lib/*.js`、带 hash 的 chunk）上——升级上游 = 换 commit + 按新版本号重写补丁文件。

### 17 个补丁分类

| 类别 | 补丁 | 说明 |
|---|---|---|
| Windows 隐藏控制台（6） | `dsh`、`dsh-subprocess-local`、`dsh-sandbox-windows-acl`、`open`、`dsh-web-app` | 子进程加 `windowsHide: true` / `STARTF_USESHOWWINDOW`。**我们 Rust 侧 `CREATE_NO_WINDOW` 已解决，一个都不需要** |
| 桌面功能钩子（3） | `dsh-web-app`（openBrowser:false）、`dsh-client-ui-workspace`（拖放标记）、`dsh-client-ui-directory-picker-browse`（原生选择器按钮） | 桌面专属 UI/行为 |
| 上游 bug 修复（6） | `dsh-agent-loop`（空 tool name 优雅失败）、`dsh-llm-deepseek`（容忍空 id/name）、`dsh-token-meter`（负 token 防御）、`dsh-app-boot`（空 patch 层容忍）、`dsh-client-runtime`（会话选择竞态）、`dsh-host-directory-picker-browse`（Windows 重解析目录） | 对我们有意义的约 3 个 |
| 功能增强（2） | `dsh-host-apiproxy`（inputModalities）、`dsh-client-ui-trajectory`（中文本地化） | — |
| 打包工具（2） | `app-builder-lib`（keychain 密码 + NSIS 长路径）、`dshmarket` | — |

### 防退化三板斧

1. **双断言测试**：每个补丁同时断言「补丁文件含 marker」+「安装后 node_modules 内容含 marker」（tests/package.spec.ts:159-173）；
2. **CI 门禁**：`scripts/verify-layout.mjs` 强制子模块 commit == upstream.json、工作区干净、每个 `@deepseek-ai/dsh-*` 依赖精确 == `runtimePackageVersion`（:62-110）；
3. **分发校验**：`asar: true` + `asarUnpack: node_modules/**`，`verify-packaged-runtime.ts` 检查补丁目标文件物理存在（ASAR 内路径无法被符号链接引用）。

## 四、桌面能力如何暴露给 Web UI（四类通道）

1. **同源 HTTP 路由（主力）**：desktop-shell 在 harness webserver 注册私有路由（设置/档案/终端/重启/诊断…），严格 loopback + 同源校验（index.ts:249-292；desktop-settings-route.ts:89-103）；
2. **渲染端 client 插件 + URL marker**：几何服务 `desktopWindow`、settings 区块、boot 健康上报、拖放、目录选择桥——**client 与 Host 之间是普通 fetch，没有 preload/IPC 通道**（docs/plugin-services.zh.md:45）；
3. **浏览器访问门禁**：随机 token 头 `x-dsh-desktop-renderer` 注入渲染进程每个请求，外部浏览器默认 403（desktop-browser-access.ts:7-48；webserver.ts:62-94）；
4. **极窄 preload 桥**：只暴露 `getPathForFile`（拖拽取 OS 路径）（preload.ts:6-10）。

**webserver 是同一台**：`DesktopWebServer extends 上游 WebServer`，profile 组合把 harness 的 webserver 行替换为子类，只加端口冲突重试与浏览器门禁（webserver.ts:49-118；profile.ts:966-1005）。

## 五、与 DSH-desk 的差异对比

| 维度 | dsh-desktop | DSH-desk（我们） |
|---|---|---|
| 壳技术栈 | Electron（Node/TS） | Tauri 2（Rust）+ WebView2 |
| harness 进程 | **同进程**（Electron main 内 `boot()`） | **子进程**（sidecar，壳监督） |
| 壳与 harness 关系 | 壳 = 插件，在插件树内部 | 壳 = 监督器，在进程外部 |
| Node 运行时 | 复用 Electron RunAsNode | 自带 node.exe（sidecar-dist） |
| 上游交付 | npm 包 + 17 补丁 + 源码子模块 | 完整依赖树打进 sidecar tar |
| 上游代码改动 | 补丁层（可审阅、随版本换） | 零改动（纯打包） |
| profile | 多 profile + 托盘切换 + 恢复 checkpoint + 安装向导 | 单 desktop profile |
| profile 初始化 | `PROFILE_TEMPLATES.web` 直接初始化 bundles | `ensure_profile_init`（2026-08-26 已对齐同思路） |
| 桌面→Web 通道 | 同源 HTTP 路由 + 公开服务契约 | `window.__DSH_DESK__` IPC 桥 |
| 模块解析 | 自写 resolve hook 显式统一 | 依赖 harness 默认解析（曾踩双副本坑，已治本） |
| 容错 | Host 崩 = 应用崩，走恢复窗口 | sidecar 崩 = 壳重启，桌面无恙 |
| 升级 | 换 commit + 重写补丁 + verify 门禁 | sidecar:assemble 重打包，版本号变化自动刷新 |
| 包体量 | ~100MB+（Electron） | 壳 ~10MB + sidecar |

## 六、关键文件索引（本地副本）

```
examples/dsh-desktop-master/
├─ upstream.json                         # 上游 pin（commit/版本双记录）
├─ package.json:16-50                    # resolutions：17 个 patch: 补丁声明
├─ patches/                              # 17 个手工补丁（npm 构建产物）
├─ .yarn/patches/                        # 2 个 yarn patch-commit 生成补丁
├─ scripts/verify-layout.mjs             # 上游 pin / 版本族 / 边界 CI 门禁
├─ dsh-plugin-desktop/
│  ├─ package.json:139-255               # 直接依赖 ~90 个 @deepseek-ai/dsh-* 0.1.1-rc.2
│  ├─ cordis.patch.yml                   # 桌面插件行（desktop-shell 等 7 行）+ web-runtime 改写
│  ├─ src/bin.ts:69-101                  # node → Electron 启动器
│  ├─ src/main.ts:812-944                # 同进程 boot() 调用（核心）
│  ├─ src/profile.ts:724-1043            # prepareDesktopProfile：三要素组装
│  ├─ src/module-resolution.ts:47-141    # 裸包名解析 hook（防双副本）
│  ├─ src/startup-generation.ts          # generation 生命周期
│  ├─ src/index.ts:193-419               # desktop-shell Host 插件
│  ├─ src/webserver.ts:49-118            # WebServer 子类（门禁 + 端口重试）
│  ├─ src/desktop-runtime-environment.ts # pnpm/dsh shim（Electron RunAsNode）
│  ├─ src/desktop-cli.ts                 # dsh CLI bootstrap（RunAsNode）
│  ├─ tests/package.spec.ts              # 补丁双断言测试
│  └─ docs/plugin-services.md            # 公开服务契约（desktopProfiles/desktopPnpm/desktopWindow）
└─ .agents/notes/implemented/process/2026-08-15-pinned-upstream-and-isolated-yarn-workspace.md
```

## 七、对本项目的直接启示

详细演进方案见 [`docs/plans/2026-08-26-plugin-first-architecture.md`](../plans/2026-08-26-plugin-first-architecture.md)。摘要：

1. **已对齐**：web-app 只登记 bundles、不 pnpm add（2026-08-26 治本）；
2. **可借鉴**：桥服务契约化（公开服务 + 同源路由）、健康 checkpoint、启动分档、轻量补丁机制（克制使用，双断言测试）、渲染健康门禁；
3. **不照抄**：同进程方案（牺牲容错）、Electron 特有补丁（我们不需要）、三种窗口呈现模式（WebView2 是另一套技术活）。
