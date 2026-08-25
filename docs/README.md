# DSH-desk

DeepSeek Harness Web UI 的 Windows 桌面分发：**用户不装 Node、不用命令行，双击即用**。

- 壳：Tauri 2（Rust）+ 系统 WebView2，不含前端；
- 宿主：harness 以 **sidecar** 随包分发（自带 Node + pnpm standalone + 钉死版本的 `@deepseek-ai/dsh` 依赖树）；
- 复用：WebView 直接加载 sidecar 的 `http://127.0.0.1:<port>`，现有 Web UI、客户端插件、Slot/主题生态 100% 复用；
- 桥接：npm 外部插件 `@cjiaojiao/dsh-desk-bridge` 只做特性检测 + Tauri IPC 桥（`window.__DSH_DESK__`），浏览器里自动退化。

设计规格：[`docs/specs/2026-08-19-desktop-shell-design.md`](specs/2026-08-19-desktop-shell-design.md)；实施计划：[`docs/plans/2026-08-19-implementation-plan.md`](plans/2026-08-19-implementation-plan.md)。

> 状态：阶段 1（脚手架）、阶段 2（sidecar 组装与监督）已完成；阶段 3（profile 初始化与首次引导）进行中。

## 仓库布局

```
DSH-desk/
├─ apps/desktop/                # Tauri 2 壳
│  ├─ dist/                     # 等待页/错误页（本地静态资产，随仓库提交）
│  └─ src-tauri/                # Rust：监督任务、IPC 命令、托盘、桥注入
│     └─ sidecar-dist/          # sidecar 产物（生成物，不入库）
├─ crates/desk-core/         # 纯逻辑 crate（无 tauri 依赖）：监督状态机、端口、路径、日志
├─ packages/dsh-desk-bridge/ # 桥接插件（npm 发布；宿主半部 + 浏览器半部）
├─ scripts/                     # sidecar 组装、测试
└─ docs/                        # 规格、计划、本文档
```

## 开发环境要求

| 依赖 | 版本 | 用途 |
|---|---|---|
| Node + pnpm | Node 22+（推荐 24）、pnpm 10+ | bridge 构建、脚本 |
| Rust（MSVC 目标） | stable | 壳与 desk-core |
| VS Build Tools / VS 2019+ | 含 C++ 桌面工作负载 | 链接器 |
| WebView2 Runtime | Evergreen | 运行壳（Win10/11 一般自带） |

国内网络建议给 cargo 配 rsproxy 镜像（`~/.cargo/config.toml`）：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
```

## 初始化

```powershell
cd DSH-desk
pnpm install                 # Node 依赖（bridge、tauri CLI）
cargo test -p desk-core   # 顺带拉取并编译 Rust 依赖
```

## 组装 sidecar

sidecar = Node 运行时 + pnpm standalone + 钉版 `@deepseek-ai/dsh` 依赖树，一条命令可复现组装：

```powershell
pnpm sidecar:assemble          # 幂等：版本一致时跳过下载与安装
node scripts/assemble-sidecar.mjs --force   # 强制重建
```

- 产物：`apps/desktop/src-tauri/sidecar-dist/`（约 330MB：node 101 + 依赖树 211 + pnpm 18）；
- 下载缓存在 `.downloads/`（可手动把 `node-v*-win-x64.zip`、`pnpm-*.tgz` 放进去加速）；
- **版本钉死常量**在 `scripts/assemble-sidecar.mjs` 顶部（`NODE_VERSION` / `PNPM_VERSION` / `DSH_VERSION`），升级 harness = 改 `DSH_VERSION` 后重新组装 + 发新版安装包。

## 初始化 desktop profile（开发期手动）

壳在首启会全自动完成此步（阶段 3）；开发期可手动预演：

```powershell
$side = "D:\project\open-source\DSH-desk\apps\desktop\src-tauri\sidecar-dist"
$env:Path = "$side\pnpm;" + $env:Path

# 注意必须钉版本（与 scripts/assemble-sidecar.mjs 的 DSH_VERSION 一致）
& "$side\node\node.exe" "$side\node_modules\@deepseek-ai\dsh\lib\bin.js" plugin --profile desktop add "@deepseek-ai/dsh-web-app@0.1.1-rc.2"
```

初始化后 `~/.dsh/profiles/desktop/pnpm-workspace.yaml` 需要 `allowBuilds`（koffi/node-pty），否则 pnpm 以非零码退出。最终 profile 的 `package.json` 应含：

```json
"dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
```

## 启动（开发）

```powershell
# 方式 A：tauri dev（有 watcher）
pnpm desktop:dev

# 方式 B：直接跑构建产物（更快，无 watcher）
D:\project\open-source\DSH-desk\target\debug\dsh-desk.exe
```

开发构建会自动用源码目录 `apps/desktop/src-tauri/sidecar-dist` 作为 sidecar（无需再设 `SIDECAR_ROOT`）；仍可用 `$env:SIDECAR_ROOT = "..."` 覆盖为任意目录（冒烟/调试用）。

启动序列：解析 sidecar → 初始化 desktop profile（幂等：web-app 钉 sidecar 同版本、bridge 缺则补）→ 选空闲端口 → 拉起 sidecar（`dsh --profile desktop --port N --no-open`，环境注入 `DSH_TELEMETRY_DISABLED=1` 与自带 pnpm 的 PATH）→ 等 stdout 的 `dsh web: http://…` 就绪行 → WebView 自动导航。崩溃按 1s/2s/4s 退避重启（3 次上限），耗尽后展示错误页（重试 / 打开日志目录 / 退出）。托盘菜单：显示/隐藏、重启宿主、打开日志目录、退出。

生产包用打包的资源目录（`resources/sidecar-dist`），无需设置 `SIDECAR_ROOT`。

## 测试

```powershell
cargo test -p desk-core     # 监督状态机/就绪解析/路径/日志（37 例）
cargo clippy -p desk-core -p dsh-desk --all-targets -- -D warnings
pnpm bridge:test               # 桥接插件退化契约（vitest，jsdom）
pnpm bridge:build              # lib/index.js（宿主）+ lib/client.js（浏览器 IIFE）
pnpm test:scripts              # 组装脚本单测（node:test，全离线）
```

## 打包（安装包）

```powershell
pnpm sidecar:assemble                                   # 先组装 sidecar
pnpm --dir apps/desktop tauri build --bundles nsis       # NSIS 每用户安装包
```

- 产物：`apps/desktop/src-tauri/target/release/bundle/nsis/*-setup.exe`；
- 安装包免管理员（`installMode: currentUser`），WebView2 缺失时自动引导安装（`downloadBootstrapper`）；
- portable zip 组装脚本（`scripts/build-portable.mjs`）与 tag 触发的 GitHub Releases 流水线在阶段 5 落地；
- v1 不签名（SmartScreen 见「常见问题」）。

## 日志

壳与 sidecar 的滚动日志（1MB×2）在 `~/.dsh/desktop/logs/`（尊重 `DSH_HOME`）：

- `shell-YYYYMMDD.log` —— 壳事件（启动、就绪、重启、失败原因）；
- `sidecar-YYYYMMDD.log` —— sidecar 原始输出。

托盘「打开日志目录」或错误页按钮可直达。

## 常见问题（开发机实测）

- **cargo 报 `SEC_E_NO_CREDENTIALS` / npm 下载极慢**：schannel 系工具（cargo/curl）在某些网络环境不可用；Node 系（pnpm/脚本）走 OpenSSL 不受影响。cargo 配 rsproxy 镜像 + 完整访问权限即可；仓库根 `.npmrc` 已放宽 pnpm 下载超时。
- **`link.exe not found`**：默认 MSVC 工具链缺 C++ 链接器。装 VS 2022 Build Tools（「使用 C++ 的桌面开发」工作负载）并 `rustup default stable-x86_64-pc-windows-msvc` 即可。详见 `docs/rust-install.zh.md`。
- **`dlltool.exe not found`**：切到 `x86_64-pc-windows-gnu` 后的连环坑——rustup 自带 MinGW 是精简版（缺 `as.exe`），`windows-sys` 的 raw-dylib 无法生成导入库。本项目只支持 MSVC 工具链，切回即可。详见 `docs/rust-install.zh.md`。
- **`dsh plugin add` 失败 404 `dsh-frontend`**：历史版本里 npm `latest` 曾指向废弃版本；务必显式钉版本（与 `DSH_VERSION` 一致，当前 `@0.1.1-rc.2`，见上）。
- **pnpm 报 `ERR_PNPM_IGNORED_BUILDS`**：profile 与 sidecar 的 `pnpm-workspace.yaml` 都要有 `allowBuilds`（koffi/node-pty）；koffi 的二进制经 optional 依赖 `@koromix/koffi-win32-x64` 分发，其 install 脚本检测到二进制后自动跳过编译，无需本机 C++ 工具链。
- **组装脚本缺 `--no-optional` 曾致宿主启动失败**：sharp/koffi 的平台二进制都走 optionalDependencies，安装必须保留 optional。
- **tauri dev 反复重建**：watcher 会把 330MB 的 `sidecar-dist` 当源码监控；开发期建议用方式 B（直接跑 exe），watcher 忽略配置待优化。
- **与既有 dsh CLI 共存**：桌面永远用自己的 sidecar，不检测、不借用、不升级用户系统里的 dsh；共享的只是 `~/.dsh` 用户数据（profile、会话、凭证）。
- **SmartScreen（未签名）**：安装时选「更多信息 → 仍要运行」；README 用户版（阶段 6）会给截图步骤。

## 不变式

开源、自部署、无中心服务器；**零遥测**（sidecar 强制 `DSH_TELEMETRY_DISABLED=1`，现在不做、以后也不做）；发布唯一渠道 GitHub Releases；插件分发走公共 npm registry；模型用 DeepSeek 官方 API（用户自己的 key）；不削弱 harness 现有信任模型（沙箱/审批栈完全宿主执行，桌面只做通知镜像）。
