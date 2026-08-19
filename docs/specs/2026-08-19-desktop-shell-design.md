# dsh-desktop 设计规格：DeepSeek Harness Web UI 的 Windows 桌面分发

- 日期：2026-08-19
- 状态：已确认（头脑风暴定案）
- 仓库：`dsh-desktop`（独立仓库，本文档即仓库内规格）

## 1. 目标与不变式

把 DeepSeek Harness Web UI 扩展为面向普通用户的 Windows 桌面产品：用户不装 Node、不用命令行，双击安装即用。

**永久不变式（产品基线）**：

1. **开源、用户自部署、无中心服务器**。产品全链路无自有服务器：
   - 发布渠道 = GitHub Releases（开源仓库本身）
   - 插件分发 = 公共 npm registry
   - 模型 = DeepSeek 官方 API（用户自己的 key）
2. **零遥测，现在不做，将来也不做**。不采集任何遥测/统计/崩溃上报。网络出站流量只存在于用户主动触发的动作：调用 DeepSeek API、从 npm registry 装插件、用户发起的 web 搜索等。日志只落本地文件，用户反馈 bug 时自行选择提供。
3. **不削弱 harness 现有信任模型**。沙箱模式（read-only / workspace-write / danger-full-access）与审批栈完全由宿主执行，桌面壳不参与、不代理、不静默放行。

## 2. 背景：现有架构事实

- `dsh web` 启动 Node 宿主进程（Cordis 插件树），内置 web 服务器，向浏览器注入 `window.__DSH_BOOT__`（入口图：插件清单 + client bundle 列表）。浏览器端壳 `@deepseek-ai/dsh-client-web` 按图加载 UI 与客户端插件。
- Web UI 是重度插件化资产：聊天壳 + Slot 系统 + 主题 + 动态 Cordis 插件（SSH 面板、任务看板、右侧面板等均挂在浏览器客户端 Slots）。任何抛弃浏览器壳的方案都会使该生态失效。
- 浏览器与宿主通过 `dsh-client-connection` 走 RPC（`/api` Typert 网关）+ 事件流；存在 trusted-host 防 DNS-rebinding 栅栏。
- `apps/web` 已带 PWA manifest（`display: fullscreen`），可被浏览器"安装为应用"，但进程生命周期仍依赖 `dsh web`，不能独立服务于桌面分发。
- profile 机制：`$DSH_HOME/profiles/<name>/`（默认 `~/.dsh`）持有 `package.json`（`dsh.profile.bundles` 层列表 + 插件依赖）、`cordis.patch.yml` 用户补丁层与 profile 专属 `node_modules`。`dsh plugin --profile <name> add <pkg>` = 在该 profile 目录内执行 `pnpm add` + reconcile bundles。插件状态 100% 活在用户家目录，与 dsh 安装位置无关。

## 3. 路线决策

| 路线 | 结论 |
|---|---|
| A. Tauri 2 壳 + harness sidecar（系统 WebView2） | **选定**。小安装包、原生集成路径干净（无 Node ABI 耦合）、100% 复用现有 UI 与客户端插件生态、壳+sidecar 可整包原子更新。 |
| B. Electron 壳 + harness 子进程 | 不选。安装包 +150MB；内嵌宿主会与 Electron 的 Node ABI 耦合（`native/` 原生模块需 electron-rebuild）；对分发产品是实打实的体积成本。 |
| C. 原生瘦客户端 + ACP/JSON-RPC | 不选。ACP 是 automation-only，交互面需全部重写，且彻底失去客户端插件/Slot/主题生态，与"一切皆插件"的架构背道而驰。 |

首发平台：Windows（x64）。模型后端：仅 DeepSeek 官方 API，首次引导填 key。

## 4. 总体架构与进程模型（双仓库模型）

```
┌─ dsh-desktop（独立仓库） ─────────────────────────────┐   ┌─ deepseek-harness（上游，仅发布消费） ─┐
│  Tauri 2 壳（Rust）                                    │   │  现有发布管线：npm 上的                │
│   ├─ 窗口 / 托盘 / 通知 / 快捷键 / 单实例               │   │  @deepseek-ai/dsh-cli、dsh-web-app、   │
│   ├─ sidecar 监督（拉起、端口协商、重试、崩溃重启）      │◀──│  dsh-web-frontend（dist）等包           │
│   ├─ 桥接插件包（packages/dsh-desktop-bridge）         │   └───────────────────────────────────────┘
│   └─ 首次运行初始化 ~/.dsh/profiles/desktop            │
│      拉起: node dsh web --profile desktop --port <壳选定的空闲端口>
│      WebView2 加载 http://127.0.0.1:<port>             │
└────────────────────────────────────────────────────────┘
```

**核心决策**：

1. **壳不含前端**。Tauri 窗口直接加载 sidecar 的本地 HTTP 地址；Web UI、Slot 插件体系、主题、动态 Cordis 插件零改动复用。`apps/web` 的 Vite dist 作为 sidecar 资产随包分发。
2. **端口协商（零上游侵入）**：壳选定空闲端口，以 `--port N` 传给 `dsh web`；绑定失败换端口重试。就绪信号 = 现有 stdout 的 `dsh web: http://…` URL 行做健康检查。harness 仓库 v1 无需任何改动；`--port-file`/`--token-file` 等加固留作后续可选上游 PR。
3. **监督状态机（壳内）**：`spawn → 等 URL 行就绪 → 加载 WebView → 运行`；sidecar 崩溃时退避重试 3 次，仍失败则展示壳内本地错误页（重试按钮 + 打开日志目录），此时托盘"退出"可终止进程。
4. **更新单元**：壳 exe + sidecar 目录是**一个原子产物**，单版本号同时烙印两者。
5. **单实例**：二次启动不拉起新进程，只聚焦已有窗口。

**组件清单**（dsh-desktop 新增面）：

| 组件 | 位置 | 职责 |
|---|---|---|
| Tauri 壳（Rust） | `apps/desktop/src-tauri` | 窗口/托盘/通知/快捷键/监督/单实例 |
| 桌面核心 crate | `crates/desktop-core` | sidecar 监督、端口协商、profile 初始化、日志 |
| 桥接插件 | `packages/dsh-desktop-bridge`（npm 发布） | 在 Web UI 内暴露桌面能力（原生文件夹选择、通知镜像、窗口控制），特性检测式，无则退化 |
| sidecar 组装脚本 | `scripts/` | Node 运行时下载、pnpm 钉版安装、裁剪 |

**数据流**：用户双击 → 壳（单实例检查）→ 拉起 sidecar → 宿主就绪 → WebView 加载 → 浏览器与宿主间沿用现有 RPC/事件流；原生能力经 Tauri IPC（壳注入 `window.__DSH_DESKTOP__` 桥）暴露给桥接插件，插件再以 Slot 方式融入 UI。**宿主核心零改动**。

## 5. Profile 策略与插件同步

**独立 `desktop` profile（`~/.dsh/profiles/desktop/`）**，自包含、可预测。壳解析 harness 家目录时尊重 `DSH_HOME` 环境变量（存在时以它为准，缺省 `~/.dsh`），与宿主行为一致。

**首次运行初始化**：桌面端用自带 node + pnpm standalone 依次执行（全程走公开 CLI 命令面，不碰内部 API）：

1. `dsh plugin --profile desktop add @deepseek-ai/dsh-web-app`（隐式初始化 profile）
2. `dsh plugin --profile desktop add @linxin666/dsh-desktop-bridge`（注册桥接插件；npm 包名提案，以实际发布 scope 为准）

reconcile 机制自动写入 `dsh.profile.bundles`，最终层栈 = `dsh-base` + `dsh-web-app` + `dsh-desktop-bridge`。初始化幂等，失败可安全重跑。

**web → desktop 单向同步功能**（设置内）：读取 `~/.dsh/profiles/web/` manifest，与 desktop profile 对比列出差异，用户勾选后逐个以 `dsh plugin --profile desktop add <pkg>` 导入。为已有 web 插件集的开发者提供迁移路径；不做双向镜像。

**与既有 dsh 安装共存**：

- **用户没有 dsh**：桌面 App 不安装系统级 dsh，也不需要。sidecar = 随包 Node 运行时 + 钉死版本的 `@deepseek-ai/dsh-*` 包 + pnpm standalone，装在 App 资源目录，全程无 PATH/npm/全局命令。v1 明确不做"把 dsh 装进用户全局 npm"。
- **用户已有 dsh**：完全共存、互不调用。桌面永远用自己钉版本的 sidecar，不检测、不借用、不升级用户系统里的 dsh；反之亦然。共享的是用户数据（`~/.dsh`），不是程序。桌面首次引导检测到 `~/.dsh/profiles/web` 时主动提示可导入。开发者可用自己 CLI 的 `dsh plugin --profile desktop add …` 管理桌面插件，与内置安装 UI 写同一份存储。
- **已知技术细节**（文档注明，v1 接受）：
  - `~/.dsh/profiles/node_modules` 的 peer 回退符号链接由"最后一次启动的安装" heal 指向自己的包（last-writer-wins）。
  - 会话格式 pre-release 无兼容承诺，建议桌面 sidecar 与用户 CLI 保持相近 rc 版本。
- **插件生效时机**：profile 在宿主启动时解析，新装插件在下次宿主启动生效；桌面提供"重启宿主"动作（只重启 sidecar，不重启 App）。`cordis.patch.yml` 用户补丁层本身热重载。

## 6. 安全边界

1. **不削弱现有信任模型**：sidecar 只绑 127.0.0.1 + 随机端口；现有 `/api` trusted-host 防 DNS-rebinding 栅栏原样生效。桌面端与浏览器 `dsh web` 在此面上完全一致。
2. **可选 v1.5 加固**（上游小 PR）：per-boot 随机令牌（`--token-file`），壳以 header 注入、`/api` 校验。对浏览器模式同样有益；不做则 v1 沿用现状，不影响发布。
3. **权限模型零改动**：沙箱与审批栈完全由宿主执行；审批提示照常在 Web UI 呈现，桌面只做通知镜像（点通知聚焦窗口），审批动作永远不落到壳里。
4. **凭证**：API key 走现有凭证机制（`~/.dsh` 的 .env），首次引导写同一位置；不写注册表、不进日志、不随打包产物分发。
5. **无更新供应链**：v1 无 updater、无 update manifest、无任何更新请求。签名证书不阻塞发布；后续若做签名，只服务于 SmartScreen 信任，与更新机制无关。
6. **WebView 硬化**：生产包禁外部导航（外链交系统浏览器）、不注入 devtools、不关闭任何安全开关；WebView2 用 Evergreen（随系统自动更新）；Tauri IPC 仅对 `127.0.0.1:<port>` 域白名单开放。
7. **日志隐私**：sidecar 日志滚动写本地文件，错误页提供"打开日志目录"；零遥测（见第 1 节不变式）。

## 7. 原生能力清单（Windows v1）

| 能力 | 形态 | 备注 |
|---|---|---|
| 托盘图标 + 菜单 | 显示/隐藏、重启宿主、退出 | 关闭窗口默认退出进程；托盘驻留留 v1.5 |
| 通知 | 审批请求、长任务完成 | 仅镜像通知，动作回 Web UI |
| 全局快捷键 | 默认 `Ctrl+Alt+D` 唤起/隐藏，可改 | v1 只做唤起/隐藏 |
| 单实例锁 | 二次启动聚焦已有窗口 | Tauri 内置 |
| 原生文件夹选择 | 首次引导选工作区 + UI 内 path-picker 桥 | 经 `window.__DSH_DESKTOP__` 桥 |
| 开机自启 | 设置项，默认关 | |
| 窗口状态记忆 | 尺寸/位置持久化 | 首屏默认 1280×800 居中 |
| 日志落地 | 滚动日志 + 一键打开目录 | |

**明确不做（v1）**：`dsh://` 深链协议、托盘常驻模式、遥测（永久）、多窗口/多工作区管理、Linux/macOS 打包、原生文件系统直连（harness 自带 fs 沙箱已够）。

**桥接插件边界**：`dsh-desktop-bridge` 只做"能力探测 + 调 Tauri IPC"，不做业务逻辑；业务逻辑全部留在宿主与现有 UI。浏览器里加载同一份 UI 时桥接自动退化，行为等价。

## 8. 首次引导体验

```
双击安装包 → 装到用户目录（免管理员权限）
→ 首次启动：
   1. 单实例锁
   2. 自检 WebView2（缺失时走 Evergreen 引导安装）
   3. 初始化 ~/.dsh/profiles/desktop（自带 node+pnpm，公开 dsh plugin 命令面）
   4. 选定空闲端口 → 拉起 sidecar → 等待 URL 行就绪
   5. WebView 加载 http://127.0.0.1:<port>
→ 页面内首跑引导（复用现有 Web onboarding）：
   a. 填 DeepSeek API key（写 ~/.dsh/.env，与 CLI 用户同一凭证机制）
   b. 选默认工作区（原生文件夹对话框经 bridge；记入 ~/.dsh/desktop/config.json）
   c. 落地聊天界面
```

- **引导失败路径**：profile 初始化失败 / sidecar 起不来 → 壳内本地错误页（重试 + 打开日志目录），托盘菜单可退出进程；任何步骤可安全重跑。
- **语言**：首发 zh-CN（Web UI 默认即 zh-CN），i18n 留后续。

## 9. 分发与更新

| 环节 | 方案 |
|---|---|
| 安装包 | NSIS 每用户安装（免管理员）、x64 单架构；附带 portable zip 产物 |
| 发布渠道 | GitHub Releases（唯一渠道），tag 触发 CI 构建上传 |
| 版本 | 壳+sidecar 单版本号；sidecar 用 npm 精确钉死 `@deepseek-ai/dsh-*` rc 版本 |
| 更新 | v1 无自动更新、无 update manifest、无任何更新请求；设置内按钮文案为「查看最新版」= 打开 GitHub Releases 页面（用户主动触发的浏览器跳转） |
| 升级 | 下载新安装包覆盖安装；`~/.dsh` 用户数据（profile、会话、凭证）原样保留 |
| 签名 | v1 不签名（README 写明 SmartScreen 处理步骤："更多信息 → 仍要运行"）；证书后续可选，仅服务于信任 |
| 插件分发 | 公共 npm registry（`dsh plugin` 原生路径），与内置安装 UI 同源 |
| 支持面 | README + FAQ + 日志目录指引；issue tracker 作为反馈渠道 |

## 10. dsh-desktop 仓库布局（提案）

```
dsh-desktop/
├─ apps/desktop/              # Tauri 2 壳（Rust）+ 等待/错误页 stub
├─ crates/desktop-core/       # sidecar 监督、端口协商、profile 初始化、日志
├─ packages/dsh-desktop-bridge/  # 桥接插件（npm 发布；host 注册 + client bundle
│                               #   特性检测 + Slot：文件夹选择/通知镜像/窗口控制）
├─ scripts/                   # sidecar 组装（node 运行时下载、pnpm 钉版安装、裁剪）
├─ .github/workflows/         # Windows CI：构建 + 冒烟 + tag 发布
└─ docs/                      # README / FAQ / 安装指引 / specs
```

## 11. 关键风险与验证策略

1. **WebView2 渲染差异**：UI 本身仍被 `apps/web` 的 Playwright e2e 全量覆盖（同一份 served UI）；桌面层新增冒烟测试（CI：启动壳 → 健康检查 → 页面标题出现 → 关闭），不放重复的 UI 断言。
2. **sidecar 体积**（约 150–250MB 安装后）：桌面场景可接受；`pnpm prune` + 精简 profile 压体积，README 说明构成。
3. **pnpm standalone 的 Windows 兼容**：`dsh plugin` spawn 的是 PATH 上的 `pnpm`；桌面端把自带 pnpm 注入子进程 PATH（已有 .cmd shim 处理先例），实现期验证。
4. **端口竞争/安全软件拦截**：失败重试 + 错误页暴露日志；文档 FAQ 覆盖。

## 12. 后续（v1.5+，非承诺）

- 上游 PR：`--port-file` 端口协商、`--token-file` per-boot 令牌加固（对浏览器模式同样有益）
- 托盘常驻模式、`dsh://` 深链协议、代码签名（SmartScreen 信任）
- macOS/Linux 打包
