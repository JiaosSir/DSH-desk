# DSH-desk 实施计划：DeepSeek Harness Web UI 的 Windows 桌面分发

> **For agentic workers:** 本计划按任务逐条执行（每阶段提交一次）。执行时使用 executing-plans 方式：批内连续完成任务、阶段边界作为检查点；任何与定案冲突的实现需求先回到用户确认。
> 步骤使用 `- [ ]` 复选框跟踪。

**Goal:** 把 DeepSeek Harness Web UI 做成 Windows x64 桌面产品——用户不装 Node、双击即用；Tauri 2 壳监督随包 sidecar（钉版 harness），WebView2 直载 `http://127.0.0.1:<port>`，原生能力经 `window.__DSH_DESK__` 桥注入，浏览器中自动退化。

**Architecture:** 壳（Rust，不含前端）→ 单实例锁 → 初始化 `~/.dsh/profiles/desktop`（自带 node+pnpm，公开 `dsh plugin` 命令面）→ 选空闲端口拉起 sidecar（`dsh --profile desktop --port N`）→ 等 stdout 的 `dsh web: http://…` 就绪行 → WebView 加载；崩溃退避重启 3 次，失败展示壳内错误页。sidecar = 随包 Node + pnpm standalone + 精确钉死的 `@deepseek-ai/dsh` 依赖树；桥接插件 `@JiaosSir/dsh-desk-bridge` 是 npm 发布的外部插件（`dsh.bundle` patch + client bundle 经 `__DSH_BOOT__` 图加载），只做特性检测 + Tauri IPC 桥，零业务逻辑。

**Tech Stack:** Rust（Tauri 2 + desk-core crate，无 tauri 依赖纯逻辑 + tokio 进程监督）、Node 22 LTS + pnpm standalone（仅 sidecar 与构建期）、TypeScript（bridge 插件，tsdown 双端构建）、NSIS（每用户）+ portable zip、GitHub Actions（Windows runner）。

---

## 0. 依据与已核实的上游事实（写计划时的实测结论）

以下事实均在本机 `D:\project\open-source\deepseek-harness`（只读参考）与 `~/.dsh/profiles/web/node_modules/@linxin666/*`（已装外部插件）中核实，是计划的硬前提：

| # | 事实 | 出处 |
|---|---|---|
| F1 | 就绪信号 = stdout 行 `dsh web: http://127.0.0.1:<port>`（可能带 ` (LAN: http://…:port)` 后缀）；官方 e2e 用 `/dsh web: (http:\/\/[^\s]+)/` 提取，90s 超时 | `packages/bundle/web-app/src/index.ts:168`、`apps/web/tests/smoke-real.e2e.ts:32-51` |
| F2 | CLI 包名 `@deepseek-ai/dsh`（bin `dsh` → `lib/bin.js`），当前 `0.1.0-rc.5`；`dsh web` 是 `--profile web` 的硬别名；launcher 只解析自己的旗标，`--port N` 等内层参数原样传给 webStartup 服务 | `apps/cli/src/args.ts`、`apps/cli/package.json` |
| F3 | 桌面启动命令 = `dsh --profile desktop --port N`：web bundle 的补丁把 `--port` 经 `webStartup` 服务接入 webserver（缺省 host `127.0.0.1`、port `3080`） | `packages/bundle/web-app/cordis.patch.yml:115-120`、`packages/bundle/web-app/src/startup.ts` |
| F4 | `dsh plugin --profile <name> add <pkg>` = 首次隐式初始化 profile（模板外的名字用 `DEFAULT_PROFILE_BUNDLES = ['@deepseek-ai/dsh-base']`）→ `spawnSync('pnpm', args, { cwd: profileDir, shell: win32 })` → 按安装态 reconcile `dsh.profile.bundles`（有 `dsh.bundle` 声明的依赖进层栈） | `apps/cli/src/plugin.ts:120-158`、`packages/boot/app-boot/src/profile.ts:114-143` |
| F5 | Windows 下 `dsh plugin` 以 `shell:true` 调 PATH 上的 `pnpm`（.cmd shim）；profile 目录自带 `pnpm-workspace.yaml`（`nodeLinker: hoisted`、`autoInstallPeers: false`）→ 自带 pnpm 必须 ≥10 且注入子进程 PATH | `apps/cli/src/plugin.ts:127-133`、`packages/boot/app-boot/src/profile.ts:138-143` |
| F6 | bundle 解析双锚点：先 dsh 安装目录（= 桌面 sidecar 自己的 node_modules），后 profile 目录；`@deepseek-ai/dsh` 的依赖树含 `dsh-base`、`dsh-web-app`、`dsh-web-frontend`（dist）→ 钉死一个 `@deepseek-ai/dsh` 版本即钉死整个 web 宿主 | `packages/boot/app-boot/README.md:38`、`apps/cli/package.json:22-84` |
| F7 | 外部插件 manifest：`"dsh": { "bundle": { "patch": "./cordis.patch.yml" }, "client": { "inject": [...], "platform": "web" } }`；patch 为 `- insert: [{id, name}]`；`./client` 导出在 `/plugins/<id>/client.js` 服务；client bundle 是 `window.__ModuleLoader__.load({id, factory(require)})` 的 IIFE（tsdown 构建） | `~/.dsh/profiles/web/node_modules/@linxin666/dsh-ssh/{package.json,cordis.patch.yml,lib/client.js}` |
| F8 | 审批事件面：宿主侧 waterfall `approval/request`（旁听者必须 `next()` 委托，不答即不影响信任模型）；浏览器侧 mux 帧 `approval/requested`（含 sessionId/approvalId/toolName/reason） | `packages/interaction/user-approval/src/index.ts:30`、`packages/host/apiproxy/src/api/events.ts:72` |
| F9 | `DSH_HOME` 环境变量优先于 `~/.dsh`；`$DSH_HOME/profiles/node_modules` 平铺 peer 回退符号链接由"最后一次启动的安装" heal（last-writer-wins，规格已接受） | `packages/util/home-paths/src/index.ts:79-88`、`packages/boot/app-boot/src/profile.ts:205` |
| F10 | harness engines `node: ^22.19.0 || >=24.0.0`；遥测开关 `DSH_TELEMETRY_DISABLED`（任意非空即禁用会话遥测行） | 根 `package.json:8-9`、`apps/cli/src/profile-boot.ts:80-83` |

## 1. 已定决策（不得重开）与本次新增的实现决策

已定决策 = 用户提示词第 1-12 条 + 规格文档，本计划全部遵守。计划在规格允许的空白处**新增**以下实现决策（如用户不认可请指出）：

| ID | 实现决策 | 理由 |
|---|---|---|
| D1 | sidecar 以 `bundle.resources` 整目录打包（`resources/sidecar-dist/{node/,pnpm/,node_modules/}`），壳用 `std::process` 直接拉起，不用 tauri-plugin-shell 的 externalBin | externalBin 只接受单文件且强制加 target-triple 后缀；目录树 + 自管 stdout/环境更适合监督场景 |
| D2 | sidecar 环境强制 `DSH_TELEMETRY_DISABLED=1` | 落实"零遥测"不变式；harness 自带开关，非上游改动 |
| D3 | sidecar 启动命令：`<sidecar>/node/node.exe <sidecar>/node_modules/@deepseek-ai/dsh/lib/bin.js --profile desktop --port N`，cwd = profile 目录 | 绕开 .cmd shim；lib/bin.js 是发布产物中的稳定入口 |
| D4 | 日志落 `~/.dsh/desktop/logs/`（尊重 DSH_HOME），sidecar 与壳各一个滚动文件（1MB×2）；错误页/托盘/设置均有"打开日志目录" | 规格 §8b 已把桌面自有状态放在 `~/.dsh/desktop/`，日志同址便于发现 |
| D5 | 等待/错误页是壳内置静态资产（`apps/desktop/dist/{index.html,error.html}` 作为 frontendDist），就绪后 `webview.navigate(url)` 切到 sidecar | 壳不含前端，但必须有无 sidecar 时的本地错误面 |
| D6 | bridge 的 web→desktop 插件同步、重启宿主等**所有** dsh 命令由壳执行（desk-core 模块），bridge 只调 IPC；同步差异清单也由壳读两份 profile manifest 计算 | 规格决策 5："桥接插件业务逻辑零" |
| D7 | bridge 包名 `@JiaosSir/dsh-desk-bridge`（规格提案；发布前以实际 scope 为准），版本与壳同号；**bridge 不打包进 sidecar**——首启由壳执行 `dsh plugin --profile desktop add @JiaosSir/dsh-desk-bridge@<壳版本>` 钉死安装（规格决策 4/5），开发期用 env `DSH_DESK_BRIDGE_SPEC=link:<绝对路径>` 覆盖为本地链接；发布管线先 npm publish bridge 再出安装包 | 同一原子产物单版本号（规格决策 4）；bridge 与用户插件同驻 profile、同走 npm 分发 |
| D8 | 首次引导在页面内做（复用 web onboarding）：API key 写 `~/.dsh/.env`（壳写文件）、工作区经原生文件夹对话框选定后写 `~/.dsh/desktop/config.json` | 规格 §8 |
| D9 | CI 冒烟 = 壳 `DSH_DESK_SMOKE=1` 自测模式：初始化 profile → 拉起 sidecar → 等就绪 → GET `/` 断言 200 与标题 → 退出 0；不开窗口，不依赖 GUI 会话 | 规格 §11.1"桌面层新增冒烟测试"的落地形态 |
| D10 | Node 钉 22 LTS 最新（≥22.19）；pnpm standalone 钉 10.x 最新；harness 钉 `0.1.0-rc.5`（计划时最新，实现期以组装脚本常量统一升级） | 规格"钉死版本" |
| D11 | 全局快捷键 v1 从 `~/.dsh/desktop/config.json` 的 `hotkey` 读取（缺省 `Ctrl+Alt+D`，非法值回退缺省）；设置 UI 显示当前值，改键 UI 留 v1.5 | 规格 §7"可改"最小实现；避免 v1 引入改键 UI 面 |
| D12 | NSIS `installMode: currentUser`（每用户免管理员）；portable zip 由脚本打包 `target/release` 的 exe + resources 目录；v1 不签名 | 规格 §9 |

## 2. 目标仓库布局

```
DSH-desk/
├─ apps/desktop/                        # Tauri 2 壳
│  ├─ package.json                      # @tauri-apps/cli 等 devDeps
│  ├─ dist/                             # 等待页/错误页静态资产（frontendDist）
│  │  ├─ index.html                     # "正在启动…"页
│  │  └─ error.html                     # 错误页（重试/日志/退出）
│  └─ src-tauri/
│     ├─ Cargo.toml                     # tauri + desk-core + 插件
│     ├─ tauri.conf.json                # 窗口/托盘外置/资源/NSIS/WebView2
│     ├─ build.rs
│     ├─ capabilities/default.json      # IPC 能力（限 127.0.0.1 远程域）
│     ├─ icons/…                        # 应用图标
│     └─ src/
│        ├─ main.rs                     # 组装：插件注册、窗口、状态机接线
│        ├─ lib.rs                      # #[cfg_attr(mobile…)] run()
│        ├─ commands.rs                 # #[tauri::command]：桥的 IPC 面
│        ├─ bridge.rs                   # initialization_script 注入 __DSH_DESK__
│        ├─ tray.rs                     # 托盘菜单
│        └─ shortcuts.rs                # Ctrl+Alt+D 唤起/隐藏
├─ crates/desk-core/                 # 纯逻辑 crate（无 tauri 依赖，单测覆盖）
│  ├─ Cargo.toml
│  └─ src/
│     ├─ lib.rs
│     ├─ paths.rs                       # sidecar 路径、DSH_HOME 解析、日志目录
│     ├─ ports.rs                       # 空闲端口选择
│     ├─ ready.rs                       # 就绪行解析（regex + 超时状态机）
│     ├─ supervisor.rs                  # 监督状态机（spawn/重试/崩溃重启）
│     ├─ profile.rs                     # desktop profile 初始化（跑 dsh plugin）
│     └─ logs.rs                        # 滚动日志写入器
├─ packages/dsh-desk-bridge/         # npm 发布的外部桥接插件
│  ├─ package.json                      # dsh.bundle.patch + dsh.client manifest
│  ├─ cordis.patch.yml                  # insert 行
│  ├─ tsconfig.json / tsconfig.build.json
│  ├─ tsdown.config.ts                  # 宿主 lib/index.js + 浏览器 lib/client.js
│  └─ src/
│     ├─ index.ts                       # 宿主半部（旁听审批事件 + /api/desktop/* 路由）
│     ├─ invariant.ts
│     └─ client/
│        ├─ index.ts                    # client 入口：特性检测 + Slot 注册
│        ├─ bridge.ts                   # window.__DSH_DESK__ 协议封装（无桥即退化）
│        ├─ settings-panel.tsx          # 设置"桌面"区（自启/日志/重启宿主/查看最新版）
│        ├─ notifications.ts            # 审批事件订阅 → 通知镜像
│        └─ locales.ts                  # zh/en 文案
├─ scripts/
│  ├─ assemble-sidecar.mjs              # 下载 node zip + pnpm standalone + pnpm add 钉版 dsh
│  ├─ smoke-desktop.mjs                 # 冒烟测试驱动（DSH_DESK_SMOKE=1）
│  └─ build-portable.mjs                # portable zip 组装
├─ .github/workflows/
│  ├─ ci.yml                            # 推送到 PR：cargo test / bridge build+test / 冒烟
│  └─ release.yml                       # tag v*：发布 bridge → 组装 sidecar → 构建 → GH Release
├─ docs/
│  ├─ README.md                         # 用户安装/使用/SmartScreen 指引（zh-CN 首发）
│  ├─ FAQ.md
│  └─ plans/ / specs/
├─ package.json                         # 仓库根：pnpm workspace（packages/*）+ 脚本入口
├─ pnpm-workspace.yaml
└─ .gitignore                           # target/ node_modules/ sidecar-dist/ dist/ 等
```

**提交策略**：每阶段完成即提交（`git commit -m "feat(<phase>): …"`）；计划文档本身单独一提交。跨阶段不得出现半成品留在工作树。

---

## 阶段 1：仓库脚手架（Tauri 2 app + crates + bridge 包骨架 + CI）

**目标**：仓库具备可构建、可测试的骨架：Tauri 2 壳显示内置等待页；desk-core 空 crate 带 CI 绿灯；bridge 包骨架可 build 可 test；CI 覆盖 fmt/clippy/test/build。

**产出物**：
- 根 `package.json`、`pnpm-workspace.yaml`、`.gitignore`
- `apps/desktop/**`（Tauri 2 脚手架 + 等待/错误页静态资产 + capabilities + 图标）
- `crates/desk-core/**`（含一个示例纯函数测试，确立 TDD 流程）
- `packages/dsh-desk-bridge/**`（manifest + 空宿主插件 + 空 client bundle + 测试骨架）
- `.github/workflows/ci.yml`

### 任务 1.1：仓库根与 pnpm workspace

- [ ] **Step 1** 创建 `pnpm-workspace.yaml`：

```yaml
packages:
  - packages/*
```

- [ ] **Step 2** 创建根 `package.json`：

```json
{
  "name": "dsh-desk",
  "private": true,
  "packageManager": "pnpm@10.12.1",
  "scripts": {
    "bridge:build": "pnpm --filter @JiaosSir/dsh-desk-bridge build",
    "bridge:test": "pnpm --filter @JiaosSir/dsh-desk-bridge test",
    "sidecar:assemble": "node scripts/assemble-sidecar.mjs",
    "desktop:dev": "pnpm --dir apps/desktop tauri dev",
    "desktop:build": "pnpm --dir apps/desktop tauri build",
    "smoke": "node scripts/smoke-desktop.mjs"
  }
}
```

（`packageManager` 以实现期本机/CI 使用的 pnpm 10.x 实际版本为准，写入精确号。）

- [ ] **Step 3** 创建 `.gitignore`（至少：`node_modules/`、`target/`、`apps/desktop/src-tauri/sidecar-dist/`、`*.log`、`.env`）。**注意：`apps/desktop/dist/` 不忽略**——等待/错误页是壳的必要构建资产，随仓库提交。

- [ ] **Step 4** 验证：`pnpm install` 成功（当前无依赖，空安装）；`git status` 干净可提交。

### 任务 1.2：desk-core crate（TDD 起点）

- [ ] **Step 1** 创建 `crates/desk-core/Cargo.toml`（依赖：`tokio`（rt-multi-thread、process、io-util、time）、`regex`、`serde`+`serde_json`、`tracing`、`tracing-appender`+`tracing-subscriber`、`dirs`；dev：`tempfile`）。lib name = `desk_core`。

- [ ] **Step 2** 写失败测试 `crates/desk-core/src/ports.rs` 的测试（先建 `src/lib.rs` 声明模块）：

```rust
// ports.rs
use std::net::TcpListener;

/// 选一个 127.0.0.1 空闲端口：绑定 :0 取 OS 分配端口后立即释放。
/// 与 sidecar 启动间存在 TOCTOU 竞态——由监督重试兜底（阶段 2）。
pub fn pick_free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}
```

```rust
// 测试（同一文件 #[cfg(test)]）
#[test]
fn pick_free_port_returns_ephemeral_and_rebindable() {
    let port = pick_free_port().unwrap();
    assert!((1024..=65535).contains(&port));
    // 小概率竞态下另一进程抢走端口，故容忍绑定失败（监督会重试）
    let _ = TcpListener::bind(("127.0.0.1", port)); // 不应恐慌
}
```

- [ ] **Step 3** 运行 `cargo test -p desk-core` → 失败（模块/函数不存在）→ 实现 → 通过。

- [ ] **Step 4** 提交：`git add -A && git commit -m "chore: repo root, workspace and desk-core skeleton"`。

### 任务 1.3：Tauri 2 壳脚手架

- [ ] **Step 1** 在 `apps/desktop/` 手动创建（不用 create-tauri-app 交互式，保证布局可控）：`package.json`（devDeps：`@tauri-apps/cli@^2`）。

- [ ] **Step 2** 创建 `apps/desktop/src-tauri/Cargo.toml`：`tauri = { version = "2", features = ["tray-icon"] }`、`tauri-build = "2"`、`desk-core = { path = "../../crates/desk-core" }`、插件按阶段 2/4 需要加入（阶段 1 先只加 `tauri-plugin-opener`、`tauri-plugin-dialog`，其余在对应阶段再加）；`[build-dependencies] tauri-build = { version = "2", features = [] }`。

- [ ] **Step 3** 创建 `apps/desktop/src-tauri/tauri.conf.json`（阶段 1 版；bundle 细节阶段 5 补全）：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "DSH-desk",
  "version": "0.1.0",
  "identifier": "com.dsh.desk",
  "build": {
    "frontendDist": "../dist"
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "label": "main",
        "title": "DSH-desk",
        "width": 1280,
        "height": 800,
        "center": true
      }
    ],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.ico"],
    "resources": ["sidecar-dist/**"],
    "windows": {
      "webviewInstallMode": { "type": "downloadBootstrapper" },
      "nsis": { "installMode": "currentUser" }
    }
  }
}
```

- [ ] **Step 4** 创建 `apps/desktop/dist/index.html`（等待页）与 `error.html`（错误页占位，阶段 2 接通）：均为 zh-CN、暗色、带 `<script>` 检测 `window.__DSH_DESK__` 并在存在时提供"退出/打开日志"按钮（调用 `__DSH_DESK__.quit()/openLogs()`，不存在时按钮隐藏——纯浏览器打开也安全）。

- [ ] **Step 5** 创建 `src-tauri/src/main.rs`（阶段 1 最小版：`tauri::Builder::default().run(...)`，窗口加载 `index.html`）+ `build.rs`（`tauri_build::build()`）+ `capabilities/default.json`：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "DSH-desk 主窗口能力（远程域仅本机 sidecar）",
  "windows": ["main"],
  "remote": { "urls": ["http://127.0.0.1:*"] },
  "permissions": ["core:default", "opener:default", "dialog:default"]
}
```

- [ ] **Step 6** 生成图标（占位 PNG/ICO 放 `icons/`，`tauri icon` 命令从源图生成全套）。

- [ ] **Step 7** 验证：`pnpm --dir apps/desktop tauri dev` 打开窗口显示等待页（本机验证一次即可，不进入 CI）；`cargo check -p dsh-desk` 无错。**注意**：阶段 1 的 dev 运行不加载 sidecar，属预期。

### 任务 1.4：bridge 包骨架

- [ ] **Step 1** 创建 `packages/dsh-desk-bridge/package.json`，manifest 参照 F7 事实：

```json
{
  "name": "@JiaosSir/dsh-desk-bridge",
  "version": "0.1.0",
  "type": "module",
  "main": "lib/index.js",
  "types": "lib/types/index.d.ts",
  "exports": {
    ".": { "types": "./lib/types/index.d.ts", "default": "./lib/index.js" },
    "./invariant": { "types": "./lib/types/invariant.d.ts", "default": "./lib/invariant.js" },
    "./client": { "types": "./lib/types/client/index.d.ts", "default": "./lib/client.js" },
    "./package.json": "./package.json"
  },
  "dsh": {
    "bundle": { "patch": "./cordis.patch.yml" },
    "client": {
      "inject": [
        "@deepseek-ai/dsh-client-runtime",
        "@deepseek-ai/dsh-client-connection",
        "@deepseek-ai/dsh-client-ui-settings"
      ],
      "platform": "web"
    }
  },
  "files": ["lib", "cordis.patch.yml", "README.md"],
  "peerDependencies": { "@deepseek-ai/cordis": ">=4", "react": "^18.2.0" },
  "devDependencies": {
    "@deepseek-ai/dsh-client-connection": "0.1.0-rc.6",
    "@deepseek-ai/dsh-client-runtime": "0.1.0-rc.6",
    "@deepseek-ai/dsh-client-ui-settings": "0.1.0-rc.6",
    "@deepseek-ai/dsh-host-webserver": "0.1.0-rc.6",
    "@deepseek-ai/dsh-settings": "0.1.0-rc.6",
    "@deepseek-ai/dsh-system-prompt": "0.1.0-rc.6",
    "tsdown": "^0.22.0",
    "typescript": "~5.7.2",
    "vitest": "^3.0.0"
  }
}
```

> 版本号以实现期 npm 上的最新 rc 为准；`dsh-ssh@0.1.20` 用的正是这套依赖面。

- [ ] **Step 2** 创建 `cordis.patch.yml`（与 dsh-ssh 同构）：

```yaml
# dsh-desk-bridge bundle patch: 把桥接插件行插进 desktop profile 名单。
# 宿主半部（exports "."）在宿主进程跑（审批旁听 + /api/desktop 路由），
# 浏览器半部（exports "./client"）由 dsh.client 声明加载进 Web GUI。
- insert:
    - id: desk-bridge
      name: '@JiaosSir/dsh-desk-bridge'
```

- [ ] **Step 3** 创建最小宿主半部 `src/index.ts`（阶段 1 只注册存活；阶段 4 加审批旁听与路由）：

```ts
import type { Context } from '@deepseek-ai/cordis'

export const name = 'desk-bridge'

export function apply(_ctx: Context): void {
  // 阶段 4：旁听 approval/request 事件 + 注册 /api/desktop 路由。
}
```

- [ ] **Step 4** 创建最小 client 半部 `src/client/index.ts` 与 `src/client/bridge.ts`：

```ts
// bridge.ts —— 唯一的特性检测 + IPC 协议封装（浏览器无桥自动退化）
export interface DeskBridge {
  readonly available: boolean
  pickFolder(): Promise<string | null>
  openLogs(): Promise<void>
  openReleases(): Promise<void>
  restartHost(): Promise<void>
  setAutostart(enabled: boolean): Promise<boolean>
  getAutostart(): Promise<boolean>
  quit(): Promise<void>
  notify(title: string, body: string): Promise<void>
}

declare global {
  interface Window { __DSH_DESK__?: DeskBridge }
}

export function detectBridge(): DeskBridge | null {
  return window.__DSH_DESK__ ?? null
}
```

```ts
// index.ts
import { detectBridge } from './bridge'

const bridge = detectBridge()
if (bridge !== null) {
  // 阶段 4：注册设置区、通知镜像订阅
  console.info('[dsh-desk-bridge] desktop bridge present')
} else {
  console.info('[dsh-desk-bridge] running in plain browser — degraded, no-op')
}
```

- [ ] **Step 5** 创建 `tsdown.config.ts`（双入口：宿主 ESM `lib/index.js`、浏览器 IIFE `lib/client.js`，后者经 `window.__ModuleLoader__.load` 包装——以 dsh-ssh 构建产物为蓝本，实现期用其 `src` 反推确切配置）：

```ts
import { defineConfig } from 'tsdown'

export default defineConfig({
  entry: { index: 'src/index.ts', client: 'src/client/index.ts' },
  format: ['esm', 'iife'],
  dts: true,
  // client IIFE 输出必须形如:
  //   window.__ModuleLoader__.load({ id: "@JiaosSir/dsh-desk-bridge", factory: require => {...} })
  // 具体 banner/footer 以实现期对 dsh-web-ui 构建产物（lib/client.js 首行）的核对为准
})
```

- [ ] **Step 6** 写退化行为测试 `tests/bridge.client.spec.ts`（vitest + jsdom）：无 `window.__DSH_DESK__` 时 `detectBridge()` 返回 `null` 且 client 入口不抛错；有 mock 桥时返回同一对象。运行 `pnpm --filter @JiaosSir/dsh-desk-bridge test` → 通过。

- [ ] **Step 7** 提交：`git commit -m "chore: tauri shell scaffold and bridge package skeleton"`。

### 任务 1.5：CI 骨架

- [ ] **Step 1** 创建 `.github/workflows/ci.yml`：

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

jobs:
  desk-core:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "crates/desk-core -> target" }
      - run: cargo fmt -p desk-core --check
      - run: cargo clippy -p desk-core -- -D warnings
      - run: cargo test -p desk-core

  bridge:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: pnpm }
      - run: pnpm install --frozen-lockfile
      - run: pnpm bridge:test
      - run: pnpm bridge:build

  shell-check:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "apps/desktop/src-tauri -> target" }
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - uses: pnpm/action-setup@v4
      - run: pnpm install --frozen-lockfile
      - run: pnpm --dir apps/desktop tauri build --no-bundle   # 阶段 2 起替换为带 sidecar 的完整构建 + 冒烟
```

- [ ] **Step 2** 推送前本地跑全绿：`cargo fmt --check && cargo clippy && cargo test`、`pnpm bridge:test`、`pnpm bridge:build`。

- [ ] **Step 3** 提交：`git commit -m "ci: windows + bridge skeleton workflows"`。

### 阶段 1 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 1.1 | `cargo test -p desk-core` 通过且包含 `pick_free_port` 测试 | 命令输出 |
| 1.2 | `pnpm bridge:test`、`pnpm bridge:build` 通过；client 产物首行是 `window.__ModuleLoader__.load(` 包装 | 命令 + 读 `lib/client.js` 首行 |
| 1.3 | `pnpm --dir apps/desktop tauri dev` 打开窗口显示 zh-CN 等待页，无控制台报错 | 本机人工一次 |
| 1.4 | `tauri build --no-bundle` 成功（阶段 1 不产安装包） | 命令输出 |
| 1.5 | CI 三个 job 全绿（推 PR 验证） | GitHub Actions |

### 阶段 1 风险

| 风险 | 缓解 |
|---|---|
| 本机 Rust/MSVC/WebView2 工具链未装齐，脚手架无法本地验证 | 开发机需 VS Build Tools + Rust stable；缺失则先装（环境问题，非设计问题），CI 用 windows-latest 兜底 |
| tauri.conf.json 的字段形态（webviewInstallMode/nsis.installMode/capabilities.remote）与计划所写存在出入 | 以 Tauri CLI 对 schema 的校验为准（`tauri dev/build` 对错误配置快速失败）；脚手架第一步即用 `$schema` 引用 + 实际构建验证，验收 1.4 兜底 |
| tsdown IIFE 包装的 banner/footer 细节不能凭空保证 | 已把"对照 dsh-ssh 产物反推配置"写入 Step 5；验收 1.2 直接断言产物首行 |
| Tauri 插件版本漂移（阶段 1 只装 opener/dialog，后续再加） | 每阶段加插件时统一 `cargo add tauri-plugin-*@2`，锁 Cargo.lock |

---

## 阶段 2：sidecar 组装脚本与监督（端口协商/重试/日志）

**目标**：`scripts/assemble-sidecar.mjs` 可复现地产出 `apps/desktop/src-tauri/sidecar-dist/`（node.exe + pnpm standalone + 钉版 `@deepseek-ai/dsh` 依赖树）；desk-core 具备端口选择、就绪行解析、监督状态机（含重试与崩溃重启）与滚动日志；壳接通等待页/错误页/托盘"重启宿主/退出"。

**产出物**：
- `scripts/assemble-sidecar.mjs`
- `crates/desk-core/src/{paths,ports,ready,supervisor,logs}.rs` + 单元测试
- `apps/desktop/src-tauri/src/{commands,tray}.rs`（部分）、等待/错误页与监督接线
- CI 冒烟任务雏形（阶段 3 完成）

### 任务 2.1：sidecar 组装脚本

- [ ] **Step 1** 写失败测试 `scripts/assemble-sidecar.test.mjs`（vitest，node:test 亦可）：mock 下载与 spawn，断言 (a) 钉版常量（`NODE_VERSION`、`PNPM_VERSION`、`DSH_VERSION`）非空且 DSH_VERSION 形如 `0.1.0-rc.\d+`；(b) 产出的 `sidecar-dist/` 含 `node/node.exe`、`pnpm/pnpm.cjs`、`pnpm/pnpm.cmd`、`node_modules/@deepseek-ai/dsh/package.json`；(c) `pnpm.cmd` 内容为 `@"%~dp0..\node\node.exe" "%~dp0pnpm.cjs" %*` 且 CRLF 行尾。

- [ ] **Step 2** 实现 `scripts/assemble-sidecar.mjs`，核心流程：

```js
// 常量区（实现期统一升级点）
const NODE_VERSION = '22.20.0'        // 22 LTS 最新，≥22.19（F10）
const PNPM_VERSION = '10.x'           // pnpm ≥10（F5 的 pnpm-workspace.yaml 语义）
const DSH_VERSION = '0.1.0-rc.5'      // 钉死 harness（F2/F6）

const SIDECAR_DIR = join(root, 'apps/desktop/src-tauri/sidecar-dist')

// 1. 下载 node-vX-win-x64.zip（nodejs.org/dist），解压 node.exe + 全部内置模块到 sidecar/node/
// 2. 下载 pnpm standalone（registry.npmjs.org/pnpm/-/pnpm-<v>.tgz 解包取 package/dist/pnpm.cjs）
//    → sidecar/pnpm/pnpm.cjs；写 sidecar/pnpm/pnpm.cmd（CRLF）shim 指向 ../node/node.exe
// 3. 在 sidecar/ 写临时 package.json: { "private": true, "dependencies": { "@deepseek-ai/dsh": DSH_VERSION } }
//    用 sidecar/node/node.exe 跑 sidecar/pnpm/pnpm.cjs install --prod --no-optional --ignore-scripts=false
//    （PATH 注入 sidecar/pnpm 目录，验证 F5 的 shell:true + .cmd shim 路径）
// 4. 删除临时 package.json 的依赖声明（node_modules 保留），落一个 sidecar/VERSION.json：
//    { node, pnpm, dsh, assembledAt }
```

- [ ] **Step 3** 幂等性：已有 `sidecar-dist/` 且 `VERSION.json` 三版本一致 → 跳过下载；`--force` 重建。

- [ ] **Step 4** 运行测试通过；真实跑一次 `node scripts/assemble-sidecar.mjs`（本机验证下载+安装，产物目录 gitignore）。记录安装后体积（验收 2.5）。

- [ ] **Step 5** 提交：`git commit -m "feat(sidecar): reproducible assembly script for pinned node+pnpm+dsh"`。

### 任务 2.2：就绪行解析与监督状态机（TDD）

- [ ] **Step 1** 失败测试 `crates/desk-core/src/ready.rs`：

```rust
/// 从 sidecar 输出流里抓就绪 URL 行（F1 的官方正则同构）。
pub fn extract_ready_url(accumulated: &str) -> Option<String> {
    regex::Regex::new(r"dsh web: (http://[^\s]+)")
        .expect("constant regex")
        .captures(accumulated)?
        .get(1)
        .map(|m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_plain_ready_line() {
        let out = "boot noise…\ndsh web: http://127.0.0.1:43210\n";
        assert_eq!(extract_ready_url(out).as_deref(), Some("http://127.0.0.1:43210"));
    }
    #[test]
    fn extracts_ready_line_with_lan_suffix() {
        let out = "dsh web: http://127.0.0.1:3080 (LAN: http://192.168.1.5:3080)\n";
        assert_eq!(extract_ready_url(out).as_deref(), Some("http://127.0.0.1:3080"));
    }
    #[test]
    fn none_before_ready() {
        assert_eq!(extract_ready_url("starting…"), None);
    }
}
```

- [ ] **Step 2** 运行 `cargo test -p desk-core` → 失败 → 实现 → 通过。

- [ ] **Step 3** 失败测试 `crates/desk-core/src/supervisor.rs`（监督状态机，tokio 单测用真实 `node -e` 假 sidecar，不依赖 harness）：

```rust
pub enum SupervisorEvent { Ready { url: String }, Exited { code: Option<i32>, attempt: u32 }, Failed { reason: String } }
pub enum SupervisorCommand { Start { port: u16 }, Stop, Restart }

pub struct SupervisorOptions {
    pub ready_timeout: Duration,        // 90s（F1 同值）
    pub max_attempts: u32,              // 3（规格决策 3）
    pub backoff: [Duration; 3],         // [1s, 2s, 4s]
}
pub struct Supervisor { /* child 句柄 + 输出缓冲 + 状态 */ }

impl Supervisor {
    pub async fn start(&mut self, program: &str, args: &[&str], envs: &[(String, String)]) -> Result<String, String>;
    pub async fn wait(&mut self) -> SupervisorEvent;  // 消费就绪/退出/超时
    pub fn stop(&mut self);                            // 有意停止（kill + 等待）
    pub fn is_running(&self) -> bool;
}
```

测试用例（写失败测试先行）：
1. 假 sidecar（`node -e "console.log('dsh web: http://127.0.0.1:PORT'); setInterval(()=>{},1000)"`）→ `start+wait` 返回 `Ready{url}`；
2. 立即退出的假 sidecar（`node -e "process.exit(3)"`）→ `wait` 返回 `Exited{code:3, attempt:1}`；
3. 绑定冲突场景：对同一端口先由测试占住，假 sidecar 打印失败并退出 → 监督 `Restart` 换端口（用 `pick_free_port`）后 Ready，`attempt=2`；
4. 连续失败 3 次 → `Failed{reason}`；
5. `stop()` 后不再触发 `Restart` 逻辑（`is_running()==false`）。

- [ ] **Step 4** 实现监督：tokio `Command`，stdout/stderr 经 `BufReader` 行读入共享缓冲（同时喂 `logs::append`），`tokio::select!` 于就绪行 / child 退出 / 超时；崩溃后按 backoff 重启（重试上限 3）；输出缓冲累计供错误页显示尾部。

- [ ] **Step 5** 运行 `cargo test -p desk-core` 全绿。

### 任务 2.3：路径、日志与 profile 目录定位

- [ ] **Step 1** TDD `crates/desk-core/src/paths.rs`：

```rust
/// DSH_HOME 优先（F9）；空/空白视为未设置；否则 ~/.dsh。
pub fn dsh_home() -> PathBuf {
    match std::env::var_os("DSH_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => dirs::home_dir().expect("no home dir").join(".dsh"),
    }
}
pub fn profile_dir() -> PathBuf { dsh_home().join("profiles").join("desktop") }
pub fn desktop_config_dir() -> PathBuf { dsh_home().join("desktop") }
pub fn logs_dir() -> PathBuf { desktop_config_dir().join("logs") }
pub fn sidecar_root() -> PathBuf { /* 运行时：exe 旁 resources/sidecar-dist（Tauri resource_dir）；测试：SIDECAR_ROOT 环境覆盖 */ }
```

测试：`DSH_HOME` 设置时优先；空白回退 home；子路径拼接正确。

- [ ] **Step 2** TDD `crates/desk-core/src/logs.rs`：滚动 writer（同文件 1MB 后切 `.1`，仅保留 2 份），`append(kind: LogKind, line: &str)` 带时间戳；测试：写超过阈值触发轮转、旧内容在 `.1`。

- [ ] **Step 3** 提交：`git commit -m "feat(core): supervisor state machine, ready-line parsing, paths and rolling logs"`。

### 任务 2.4：壳接通（等待页 → sidecar → 错误页）

- [ ] **Step 1** `apps/desktop/src-tauri/src/main.rs` 组装：启动即建 Supervisor → `Start{port: pick_free_port()}` → 事件循环（channel 收 SupervisorEvent）→ `Ready{url}` 时 `webview.navigate(url)`；`Failed` 时 `webview.navigate("error.html?reason=…")`；托盘"重启宿主"→ `Restart`（重置 attempt 计数）。

- [ ] **Step 2** `commands.rs` 首批命令（阶段 3/4 扩容）：

```rust
#[tauri::command] fn desktop_retry(state: State<AppState>) -> Result<(), String>
#[tauri::command] fn desktop_open_logs(app: AppHandle) -> Result<(), String>   // opener 打开日志目录
#[tauri::command] fn desktop_quit(app: AppHandle)                               // 停 sidecar 后 app.exit(0)
#[tauri::command] fn desktop_state(state: State<AppState>) -> DesktopStatus     // 错误页显示用
```

- [ ] **Step 3** `bridge.rs`：`initialization_script` 注入 `window.__DSH_DESK__`（用 `window.__TAURI__.core.invoke` 包装上述命令；`withGlobalTauri: true`），协议与阶段 1 任务 1.4 Step 4 的 `DeskBridge` 接口逐字段对齐。

- [ ] **Step 4** `tray.rs`：TrayIconBuilder + 菜单（显示/隐藏、重启宿主、打开日志目录、退出）；窗口关闭 = `app.exit(0)`（先停 sidecar；规格 §7 注释：托盘驻留留 v1.5）。

- [ ] **Step 5** 手工验证：`pnpm sidecar:assemble` 后 `pnpm desktop:dev`：等待页 → 自动切到 Web UI；托盘重启宿主可用；杀掉 node.exe 进程模拟崩溃 → 3 次退避后错误页出现"重试/打开日志目录/退出"。

### 阶段 2 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 2.1 | `cargo test -p desk-core` 全绿（就绪解析 3 例 + 监督 5 例 + 路径 + 日志轮转） | 命令输出 |
| 2.2 | `node scripts/assemble-sidecar.mjs` 幂等；二次运行无网络下载 | 命令输出 |
| 2.3 | `sidecar-dist/VERSION.json` 三版本与常量一致；`node_modules/@deepseek-ai/dsh/package.json` version = 钉版 | 读文件 |
| 2.4 | 真实启动：等待页 → Web UI；就绪 URL 端口与壳所选端口一致（日志核对） | 手工 |
| 2.5 | 安装后 sidecar 体积记录进 README 草案（实现期记实测数） | 手工 |
| 2.6 | 模拟崩溃 → 退避重启 → 3 次后错误页；托盘"重启宿主"恢复 | 手工 |

### 阶段 2 风险

| 风险 | 缓解 |
|---|---|
| nodejs.org 下载在 CI 不稳定 | 组装脚本用 `actions/cache` 键含 NODE_VERSION；本地可断点续传（脚本用流式下载 + 临时文件原子改名） |
| pnpm standalone 从 registry 提取路径随版本变化 | 脚本按 tarball 内 `package/dist/pnpm.cjs` 递归查找，找不到即报错（fail-loud） |
| `dsh plugin` 在 Windows `shell:true` 下的 pnpm.cmd 解析差异 | 已按 F5 前置 PATH 注入 + CRLF shim；阶段 3 验收专门覆盖 |
| 监督状态机与 Tauri 事件循环的线程模型耦合 | desk-core 不依赖 tauri（纯 tokio），壳侧用 channel 桥接，单测覆盖状态机本体 |

---

## 阶段 3：profile 初始化与首次引导

**目标**：首启全自动完成 `~/.dsh/profiles/desktop` 初始化（web-app + bridge 两插件，幂等）；页面内首跑引导（API key → 工作区 → 聊天界面）落地；web→desktop 插件同步设置项；`~/.dsh/desktop/config.json` 读写；单实例锁与窗口状态记忆接入。

**产出物**：
- `crates/desk-core/src/profile.rs`（初始化 + 同步清单计算）
- `apps/desktop/src-tauri/src/{single_instance,window_state,commands}.rs` 扩容
- `packages/dsh-desk-bridge` 首个可用功能面（工作区选择桥 + 设置区）——UI 部分与阶段 4 交接
- `scripts/smoke-desktop.mjs` + CI 冒烟 job

### 任务 3.1：profile 初始化（TDD）

- [ ] **Step 1** 失败测试 `crates/desk-core/src/profile.rs`（用临时 DSH_HOME + 真实 node/pnpm shim 桩，不触网）：

```rust
pub struct ProfileInitOutcome { pub ran_adds: Vec<String> }
/// 幂等初始化：profile 目录无 package.json 时先 add web-app；
/// 然后检查 dependencies 是否含 bridge（含 @scope 精确匹配），缺则 add。
/// 两个 add 都通过 bundled pnpm + PATH 注入执行（F4/F5 语义复刻）。
/// bridge 安装 spec：env DSH_DESK_BRIDGE_SPEC 优先（开发期 link: 覆盖，D7），
/// 缺省 `@JiaosSir/dsh-desk-bridge@<compile-time 壳版本>`。
pub async fn ensure_profile_init(opts: &InitOptions) -> Result<ProfileInitOutcome, String>;

/// web→desktop 单向同步清单：读两份 profile 的 package.json dependencies，
/// 差集 = web 有而 desktop 无的包名（含版本说明），供 UI 勾选导入。
pub fn compute_sync_diff(web_dir: &Path, desktop_dir: &Path) -> SyncDiff;
```

测试：空 home → 两次 add 顺序执行且参数为 `add @deepseek-ai/dsh-web-app`、`add <bridge spec>`（桩记录 argv；缺省 spec 含 `@` 版本号）；重跑 → 无 add（幂等）；bridge 已在 dependencies → 只补缺；web profile 缺失 → `compute_sync_diff` 返回空；有差异 → 差集正确且去重。

- [ ] **Step 2** 实现（关键点：`dsh plugin` 的真实参数是 `plugin --profile desktop add <pkg>`，程序 = sidecar node.exe，脚本 = `node_modules/@deepseek-ai/dsh/lib/bin.js`；子进程 env 注入 `sidecar/pnpm` 到 PATH 头部 + `DSH_TELEMETRY_DISABLED=1`；输出逐行进日志；非零退出返回带尾部输出的错误）。

- [ ] **Step 3** 真实冒烟前置：临时 DSH_HOME 下跑 `ensure_profile_init` 一次（触网），断言 profile 的 `package.json` 里 `dsh.profile.bundles` 最终 = `['@deepseek-ai/dsh-base','@deepseek-ai/dsh-web-app','@JiaosSir/dsh-desk-bridge']`（F4 的 reconcile 语义）。此条作为**本阶段验收**，不写进常跑单测（触网）。

### 任务 3.2：首启编排与 config.json

- [ ] **Step 1** `apps/desktop/src-tauri/src/main.rs` 启动序列改为：单实例锁 → `ensure_profile_init`（失败 → 错误页，托盘可退出）→ `pick_free_port` → 监督启动 → 等待页 → 就绪导航。初始化进度写入日志 + 等待页文案（"首次运行正在准备环境…"）。

- [ ] **Step 2** `commands.rs` 增加：

```rust
#[tauri::command] fn desktop_get_onboarding(app: AppHandle) -> DesktopOnboarding
//   { workspace: Option<String>, hasApiKey: bool }
//   ← workspace 读 ~/.dsh/desktop/config.json；
//     hasApiKey = $DSH_HOME/.credentials.yaml 或 $DSH_HOME/.env 含 DEEPSEEK_API_KEY（仅布尔存在性，不读值）
#[tauri::command] fn desktop_pick_workspace(app: AppHandle) -> Option<String>
//   tauri-plugin-dialog pick_folder → 写 config.json 的 workspace 字段 → 返回路径
```

- [ ] **Step 3** config.json 结构（`~/.dsh/desktop/config.json`，serde 读写，缺省值兜底）：

```json
{ "workspace": null, "hotkey": "Ctrl+Alt+D", "autostart": false }
```

- [ ] **Step 4** 单实例：`tauri-plugin-single-instance` 注册，二次启动回调聚焦已有窗口；窗口状态记忆：`tauri-plugin-window-state`（默认 1280×800 居中由 tauri.conf 保证首屏）。

### 任务 3.3：冒烟测试与 CI

- [ ] **Step 1** 壳侧 smoke 模式：env `DSH_DESK_SMOKE=1` 时 main() 走无窗口路径——初始化 → 拉起 sidecar → 等就绪 → 用 ureq 请求 `GET {url}/` → 断言 200 且 body 含 `<title>DeepSeek Harness</title>`（已核实：`apps/web/index.html:8`）→ 停 sidecar → `println!("SMOKE_OK")` → exit 0；任何失败 `SMOKE_FAILED: {reason}` → exit 1。

- [ ] **Step 2** `scripts/smoke-desktop.mjs`：spawn 构建产物 exe，env `{ DSH_DESK_SMOKE: '1', DSH_HOME: <mkdtemp> }`，超时 5 分钟，断言 exit 0 且输出含 `SMOKE_OK`；失败时打印 sidecar 日志尾部。

- [ ] **Step 3** CI 增加 smoke job（`shell-check` 替换为：assemble-sidecar（缓存）→ `tauri build --no-bundle` → `node scripts/smoke-desktop.mjs`）。

### 阶段 3 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 3.1 | profile 单测全绿（幂等/补缺/差集） | `cargo test -p desk-core` |
| 3.2 | 真实初始化（触网一次）后 bundles = base+web-app+bridge 三层，profile 目录含 pnpm-workspace.yaml | 读 `~/.dsh/profiles/desktop/package.json`（测试 DSH_HOME） |
| 3.3 | 全新 DSH_HOME 双击首启：初始化 → Web UI 出现；二次启动秒开且复用 profile | 手工 |
| 3.4 | 已装 dsh 用户的 `~/.dsh/profiles/web` 被检测到且设置里出现导入清单（阶段 4 UI，本阶段以 `compute_sync_diff` 单测 + 手工 API 检查替代） | 单测 + 手工 |
| 3.5 | CI 冒烟 job 绿：SMOKE_OK | Actions 日志 |
| 3.6 | 窗口尺寸/位置重开记忆；单实例二次启动聚焦 | 手工 |

### 阶段 3 风险

| 风险 | 缓解 |
|---|---|
| 首启初始化耗时（下载 web-app 依赖树）+ 等待页 90s 超时不够 | 超时按"初始化阶段不计时"处理：init 完成才进监督超时；等待页显示初始化日志尾部 |
| `~/.dsh/.env` 已有 CLI 用户的 key，页面引导重复要求填写 | `hasApiKey` 检测（仅判断键存在，不读值），有则跳过填 key 步骤 |
| peer 回退 heal 的 last-writer-wins 与用户 CLI 交错启动 | 规格已接受；FAQ 说明（阶段 6） |

---

## 阶段 4：桥接插件（原生能力 + 特性检测退化）

**目标**：`@JiaosSir/dsh-desk-bridge` 成为完整外部插件：宿主半部旁听审批事件并推送 SSE；client 半部在 Web UI 内提供"桌面"设置区（开机自启、快捷键显示、打开日志、重启宿主、查看最新版）、工作区路径选择桥、通知镜像；无 `window.__DSH_DESK__` 时全量退化（等价纯浏览器行为）。开发期以 `link:` 装入 desktop profile。

**产出物**：
- `packages/dsh-desk-bridge/`（宿主半部：审批旁听 + `/api/desktop/*` 路由；client 半部：Slot 设置区 + 通知订阅 + path-picker 桥）
- 壳侧配套命令（autostart/notify/restart/releases 等）
- 通知权限与全局快捷键（Ctrl+Alt+D）接线

### 任务 4.1：前期 spike（实现依据，产出记入代码注释）

- [x] **Step 1** 从 `~/.dsh/profiles/web/node_modules/@linxin666/dsh-ssh`（含 `src/`）研读并记录：宿主半部注册 webserver 路由的确切 API（`@deepseek-ai/dsh-host-webserver` 导出）与设置区 Slot 注册 API（`@deepseek-ai/dsh-settings` 的 `installSettingsSection` 用法）；把结论写进 `packages/dsh-desk-bridge/docs/spike-notes.md`。

- [x] **Step 2** 用 `dsh-ssh` 同款 devDeps 把 bridge 的宿主半部改造成真实路由注册（先注册一个 `GET /api/desktop/health` 返回 `{ ok: true }`，client 半部 fetch 它在面板显示——作为端到端最小闭环）。

### 任务 4.2：宿主半部（审批旁听 + SSE）

- [x] **Step 1** `src/index.ts` 实现（F8 语义：**旁听者永远 `next()` 委托，绝不产生审批结果**——信任模型零触碰，这是不变式 3 的落实）：

```ts
import type { Context } from '@deepseek-ai/cordis'

export const name = 'desk-bridge'

export function apply(ctx: Context): void {
  const subscribers = new Set<(evt: DesktopEvent) => void>()
  // webserver 路由注册（spike Step 1 结论的 API）：
  // GET  /api/desktop/events  → SSE 流，推送审批事件
  // POST /api/desktop/ack     → 客户端已读确认（可选）
  ctx.on('approval/request', (req, next) => {
    for (const push of subscribers) push({ type: 'approval', toolName: req.toolName, reason: req.reason })
    return next() // 永不回答 —— 桌面只是旁观者
  })
}
```

- [x] **Step 2** 单测（vitest + cordis 测试上下文，参照 dsh-ssh 测试）：注入 mock approval/request 事件 → SSE 订阅者收到推送且 `next` 被调用；断连清理。

### 任务 4.3：client 半部（设置区 + 通知镜像 + path-picker 桥）

- [x] **Step 1** `src/client/index.ts`：`detectBridge()` 为 null → 直接返回（无 Slot、无订阅，等价纯浏览器）；否则注册设置区（"桌面"）与通知订阅。

- [x] **Step 2** 设置区（`settings-panel.tsx`，zh 文案为主、en 镜像）：
  - 开机自启开关 → `setAutostart/getAutostart`（壳侧 tauri-plugin-autostart）
  - 全局快捷键当前值展示（读 config.json，只读；改键留 v1.5，D11）
  - 「重启宿主」→ `restartHost()`（壳侧 `Supervisor::restart`；提示"新装插件下次启动生效"）
  - 「打开日志目录」→ `openLogs()`
  - 「查看最新版」→ `openReleases()`（系统浏览器开 GitHub Releases——规格 §9：用户主动触发）
  - web→desktop 插件同步：列 `compute_sync_diff`（壳算，经 IPC 取回），勾选后逐包 `desktop_sync_add(pkg)`（壳跑 `dsh plugin --profile desktop add`），完成后提示重启宿主

- [x] **Step 3** 通知镜像（`notifications.ts`）：订阅 `/api/desktop/events` SSE → 收到审批事件 → `notify(title, body)`（壳侧 tauri-plugin-notification，点通知聚焦窗口）；审批动作永远留在 Web UI（规格决策 8）。

- [x] **Step 4** path-picker 桥（已按 d7dff7e 决定移除 workspace 特性，本步不再适用）。

- [x] **Step 5** 退化测试矩阵（vitest + jsdom，mock `__ModuleLoader__`）：无桥环境不注册 Slot、不建 SSE、`detectBridge()===null`；有桥环境各按钮调对应 invoke 一次。

### 任务 4.4：壳侧配套（快捷键/自启/通知/外链）

- [x] **Step 1** `shortcuts.rs`：`tauri-plugin-global-shortcut` 注册 `Ctrl+Alt+D`（读 config.json 的 `hotkey`，非法回退缺省）→ toggle 窗口可见性。

- [x] **Step 2** `commands.rs` 扩容：`desktop_set_autostart/get_autostart`（autostart 插件）、`desktop_notify`（notification 插件）、`desktop_open_releases`（opener 开 `https://github.com/<owner>/DSH-desk/releases`，owner 实现期定）、`desktop_sync_add`、`desktop_sync_list`、`desktop_restart_host`（实现为 `desktop_retry`）。

- [x] **Step 3** WebView 硬化：`on_navigation` 拒绝非 `http://127.0.0.1:<当前port>` 的导航并移交 opener（外链走系统浏览器）；release 构建禁 devtools（`devtools: false` 生产配置）。

### 任务 4.5：开发闭环（link: 装入 + 重启宿主验证）

- [x] **Step 1** 以 `dsh plugin --profile desktop add link:<bridge 包绝对路径>` 装入开发 profile（F4 锚定语义：绝对路径原样转发 pnpm）。

- [x] **Step 2** 手工验证清单：设置区出现"桌面"；审批触发时系统通知出现；Ctrl+Alt+D 唤起/隐藏；纯浏览器开同 URL 无桌面区、无报错（文件夹选择随 workspace 特性一并移除）。

### 阶段 4 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 4.1 | spike 笔记齐备且宿主半部测试绿（旁听不答、事件透传） | `pnpm bridge:test` |
| 4.2 | 设置区五项功能逐一可用；同步导入后重启宿主生效 | 手工（desktop profile） |
| 4.3 | 审批请求 → 系统通知镜像；点击通知聚焦窗口；审批仍在 Web UI 完成 | 手工（触发一次真实审批） |
| 4.4 | 纯浏览器（无桥）打开同一 URL：无桌面设置区、无报错、其余功能等价 | 手工 |
| 4.5 | 快捷键/自启/外链转移/禁外部导航逐项验证；`npm pack` 产物含 cordis.patch.yml + lib/client.js | 手工 + 命令 |

### 阶段 4 风险

| 风险 | 缓解 |
|---|---|
| 外部宿主插件注册 webserver 路由的 API 与假设不符 | 任务 4.1 spike 先行，`/api/desktop/health` 最小闭环在写业务前验证 |
| `approval/request` 旁听若误吞事件会破坏信任模型 | 旁听器强制 `next()`（代码注释 + 单测断言 `next` 被调）；评审时重点检查 |
| 通知弹窗在 WebView 聚焦时打扰用户 | 镜像通知仅当窗口不可见时发（`is_window_visible()` 判断），实现期定夺；默认保守 |
| rc 版本漂移导致 bridge 与 sidecar 不匹配 | bridge devDeps 钉 rc 版本；发布时与 DSH_VERSION 同表升级 |

---

## 阶段 5：打包与发布（NSIS + zip + GitHub Releases workflow）

**目标**：`pnpm desktop:build` 产出 NSIS 每用户安装包与 portable zip；tag `v*` 触发的 release.yml 全自动：bridge 发布 npm → sidecar 组装 → 构建 → 冒烟 → GH Release 上传；安装/覆盖安装/卸载全流程验证；`~/.dsh` 用户数据保留。

**产出物**：
- `scripts/build-portable.mjs`
- `.github/workflows/release.yml`
- 安装包元数据（描述、图标、显示名）
- 版本号策略文档段

### 任务 5.1：打包产物

- [x] **Step 1** `tauri.conf.json` 补全 bundle 段（D12）：NSIS `installMode: currentUser`、`displayLanguageSelector: false`、安装目录缺省 `%LOCALAPPDATA%\Programs\DSH-desk`（经 `nsis/installer.nsi` 定制模板，基于 Tauri 2.11.5 官方模板改 1 行）；`webviewInstallMode: downloadBootstrapper`（WebView2 缺失时自动引导安装，规格 §8.2）。

- [x] **Step 2** `scripts/build-portable.mjs`：从 `target/release/` 收集 `dsh-desk.exe` + 同层 `sidecar-dist/`（Tauri 把 bundle.resources 平铺在 exe 旁，非 `resources/` 子目录——以真实产物为准）→ 打 zip（根目录名 `DSH-desk-portable-<version>-x64`）；校验 zip 内含 exe 与 `sidecar-dist/node/node.exe`（自写零依赖 ZIP64 中央目录解析器 `scripts/zip-entries.mjs`，条目数 69069 已实测）。

- [x] **Step 3** 验证：本机已完成静默安装（`/S`）→ 已装产物冒烟 SMOKE_OK → 覆盖安装 → 卸载，`~/.dsh` 文件数前后一致（10406）；缺省目录、卸载注册表项创建/清理均确认。干净 VM 一次留发布后人工。

### 任务 5.2：release workflow

- [x] **Step 1** `.github/workflows/release.yml`（触发 `v*` tag）：

```yaml
name: Release
on:
  push: { tags: ["v*"] }
permissions: { contents: write }

jobs:
  publish-bridge:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22, registry-url: "https://registry.npmjs.org" }
      - run: pnpm install --frozen-lockfile
      - run: pnpm bridge:test
      - run: pnpm --filter @JiaosSir/dsh-desk-bridge publish --access public --no-git-checks
        env: { NODE_AUTH_TOKEN: "${{ secrets.NPM_TOKEN }}" }

  build-release:
    needs: publish-bridge
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          $v = $env:GITHUB_REF_NAME.TrimStart('v')
          (Get-Content apps/desktop/src-tauri/tauri.conf.json) -replace '"version": "0.1.0"', "`"version`": `"$v`"" | Set-Content apps/desktop/src-tauri/tauri.conf.json
          npm pkg set version=$v --workspaces   # 桥包同号（D7）
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with: { node-version: 22 }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "apps/desktop/src-tauri -> target" }
      - run: pnpm install --frozen-lockfile
      - run: pnpm sidecar:assemble          # sidecar 组装（不含 bridge；bridge 首启经 npm 钉版装入 profile，D7）
      - run: pnpm --dir apps/desktop tauri build --bundles nsis
      - run: node scripts/build-portable.mjs
      - run: node scripts/smoke-desktop.mjs # 冒烟（DSH_HOME=临时目录）
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            apps/desktop/src-tauri/target/release/bundle/nsis/*-setup.exe
            dist-portable/*.zip
```

- [ ] **Step 2** 触发一次真实 tag（如 `v0.1.0`）验证全链路；修正后固定流程。NPM_TOKEN 在仓库 secrets 配置（只写 publish-bridge job）。

### 阶段 5 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 5.1 | 安装包双击安装到用户目录（免管理员）；首启引导完整走通 | 手工（干净环境一次） |
| 5.2 | portable zip 解压即用（含全部 sidecar），不放任何注册表/开机项 | 手工 |
| 5.3 | 覆盖安装升级：`~/.dsh`（profile/会话/凭证）原样保留 | 手工 |
| 5.4 | release.yml 从 tag 到 GH Release 全自动成功；产物含 setup.exe + zip | Actions + 下载验证 |
| 5.5 | 无任何更新请求网络流量（不变式）；设置按钮仅开 Releases 页 | 手工（网络面板） |

### 阶段 5 风险

| 风险 | 缓解 |
|---|---|
| 未签名包 SmartScreen 拦截（"更多信息→仍要运行"） | 预期行为；README 写明步骤（阶段 6）；不引入签名阻塞发布 |
| NSIS 首启 WebView2 引导安装需网络 | Evergreen bootstrapper 由微软 CDN 提供；失败时错误页提示手动安装链接（FAQ） |
| 版本号多处（tauri.conf/Cargo.toml/package.json/桥包）漂移 | release.yml 统一由 tag 派生写入；本地构建用 `--version` 覆盖 |
| 端口/杀软误报 | FAQ + 日志指引；错误页给出端口被占的明确文案 |

---

## 阶段 6：文档与 FAQ

**目标**：面向普通用户的 zh-CN 文档：安装指引（含 SmartScreen 步骤）、首次使用（API key、工作区）、与既有 dsh 共存说明、FAQ、日志目录指引、开发者文档（构建/发布/插件机制）；LICENSE 与仓库元数据齐备。

**产出物**：
- `docs/README.md`（zh-CN 为主，仓库首页即产品页）
- `docs/FAQ.md`
- `docs/INSTALL.md`（或并入 README）
- `docs/dev/BUILDING.md`（开发与发布流程，含版本号策略、sidecar 升级点 D10 清单）
- `LICENSE`（MIT，与 harness 一致）、`CONTRIBUTING.md`（可选）

### 任务 6.1：用户文档

- [ ] **Step 1** README 必备章节：是什么 / 系统要求（Win10+ x64，WebView2 自动安装）/ 安装（NSIS 与 portable 两种 + SmartScreen "更多信息→仍要运行"配图步骤）/ 首次引导（API key 从 DeepSeek 平台获取、工作区选择）/ 与 CLI dsh 共存（共享 `~/.dsh` 数据、last-writer-wins 说明、rc 版本建议）/ 卸载与升级（覆盖安装、数据保留）/ 隐私（零遥测、本地日志、出站流量清单——不变式 1/2）。

- [ ] **Step 2** FAQ 至少覆盖：SmartScreen 拦截 / 端口被占或安全软件 / WebView2 缺失 / 首启网络失败（重试 + 日志）/ 插件同步与生效时机（重启宿主）/ 日志在哪、怎么给开发者 / 快捷键冲突怎么改（config.json）/ 杀软误报。

### 任务 6.2：开发者文档

- [ ] **Step 1** `docs/dev/BUILDING.md`：本计划 D1-D12 决策摘要、sidecar 组装与版本升级点（NODE/PNPM/DSH 常量）、bridge 发布流程、release 流程、冒烟测试用法、spec/plan 指针。

- [ ] **Step 2** 提交收尾：`git commit -m "docs: user guide, faq and developer documentation"`。

### 阶段 6 验收标准

| # | 验收 | 验证方式 |
|---|---|---|
| 6.1 | 一个没用过 Node 的用户按 README 从下载到聊天全流程无卡点 | 找人实测（或至少自查按步执行） |
| 6.2 | FAQ 覆盖规格 §11 全部风险条目 + SmartScreen/升级/日志三类高频问题 | 对照清单 |
| 6.3 | 开发者按 BUILDING.md 能复现构建与发布 | 自查走查 |

### 阶段 6 风险

| 风险 | 缓解 |
|---|---|
| 文档与实际行为漂移 | 阶段 6 与阶段 5 紧邻提交；验收 6.1 全流程重走一遍 |

---

## 全局验收矩阵（规格 §7-11 → 阶段任务）

| 规格条目 | 落地任务 |
|---|---|
| §7 托盘 | 2.4（Step 4） |
| §7 通知镜像 | 4.3（Step 3）+ 4.4（Step 2） |
| §7 全局快捷键 | 4.4（Step 1） |
| §7 单实例 / 窗口记忆 | 3.2（Step 4） |
| §7 文件夹选择 | 3.2（Step 2）+ 4.3（Step 4） |
| §7 开机自启 / 日志 | 4.3（Step 2）/ 2.3 |
| §8 首次引导 | 3.1 + 3.2 + 4.3（Step 4） |
| §9 NSIS+zip / GH Releases / 无更新 / 覆盖升级 | 5.1 + 5.2 |
| §10 仓库布局 | 阶段 1 |
| §11 WebView2 差异（冒烟） | 3.3 |
| §11 sidecar 体积 | 2.1（验收 2.5 记录） |
| §11 pnpm Windows 兼容 | 2.1（Step 2 注）+ 3.1（验收 3.2） |
| §11 端口竞争/安全软件 | 2.2 + 6.1（FAQ） |
| 不变式 1（开源自部署） | 5.2 + 6.1（README 隐私节） |
| 不变式 2（零遥测） | D2 + 5.2（验收 5.5） |
| 不变式 3（信任模型） | 4.2（旁听不答）+ 4.1 |

## 执行方式（用户确认后）

按上述 6 个阶段逐阶段实现，每阶段完成即 `git commit`。阶段内任务顺序执行、测试先行；任何与规格冲突的实现需求先回用户确认。验证组合：desk-core/bridge 自动化测试 + 本机 `pnpm dsh web`（deepseek-harness 真实 GUI 行为参照）+ 壳手工冒烟（等待页→Web UI→托盘→快捷键）+ CI 全绿。
