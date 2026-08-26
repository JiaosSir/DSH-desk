# DSH-desk

DeepSeek Harness Web UI 的 Windows 桌面分发：**用户不装 Node、不用命令行，双击即用**。

- 壳：Tauri 2（Rust）+ 系统 WebView2，不含前端；
- 宿主：harness 以 **sidecar** 随包分发（自带 Node + pnpm standalone + 钉死版本的 `@deepseek-ai/dsh` 依赖树）；
- 复用：WebView 直接加载 sidecar 的 `http://127.0.0.1:<port>`，现有 Web UI、客户端插件、Slot/主题生态 100% 复用；
- 桥接：npm 外部插件 `@cjiaojiao/dsh-desk-bridge` 只做特性检测 + Tauri IPC 桥（`window.__DSH_DESK__`），浏览器里自动退化。

设计规格：[`docs/specs/2026-08-19-desktop-shell-design.md`](specs/2026-08-19-desktop-shell-design.md)；实施计划：[`docs/plans/2026-08-19-implementation-plan.md`](plans/2026-08-19-implementation-plan.md)。

> 状态：六个阶段（脚手架 / sidecar 与监督 / profile 与首启 / 桥接插件 / 打包发布 / 文档）全部完成。
> 用户文档见[根 README](../README.md)（安装、SmartScreen、首启、隐私）与 [FAQ](FAQ.md)；构建与发布流程见 [dev/BUILDING.md](dev/BUILDING.md)。

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

- 产物：`apps/desktop/src-tauri/sidecar-dist/`（解压后实测约 320MB：node 运行时 + 依赖树 + pnpm）；
- **打包资产（方案 A）**：组装收尾把 sidecar 打成**未压缩 tar** `sidecar-dist.tar` 并复制版本文件 `sidecar-version.json`（均与 sidecar-dist 并列在 `src-tauri/` 下），随 `bundle.resources` 进安装包——安装时只解 1 个大文件（压缩交给 NSIS solid LZMA，体积与平铺目录持平），应用首启再解压到本地缓存（见「打包」一节）；
- 下载缓存在 `.downloads/`（可手动把 `node-v*-win-x64.zip`、`pnpm-*.tgz` 放进去加速）；
- **版本钉死常量**在 `scripts/assemble-sidecar.mjs` 顶部（`NODE_VERSION` / `PNPM_VERSION` / `DSH_VERSION`），升级 harness = 改 `DSH_VERSION` 后重新组装 + 发新版安装包。

## 初始化 desktop profile（壳自动；开发期可手动预演）

壳在首启会全自动完成此步（幂等）：登记安装层 bundles（`dsh-base` + `dsh-web-app`，**只登记进 `dsh.profile.bundles`，绝不 `pnpm add`**——add 会把 web-app 的 90+ 依赖全家桶装进 profile 的 node_modules，宿主启动时核心插件双副本加载、Symbol 分裂、工具调用崩溃，2026-08-26 桌面 glob 工具事故即此因；web-app 版本由 sidecar 解析决定），bridge 缺则 `add`（spec = `DSH_DESK_BRIDGE_SPEC` 环境变量优先，缺省 `@cjiaojiao/dsh-desk-bridge@<壳版本>`）；历史残留（dependencies 里的 web-app / 废弃包名 `@JiaosSir/dsh-desk-bridge`）自动迁移清理。首次 add 时自动修复 `pnpm-workspace.yaml` 的 `allowBuilds` 占位符（`koffi: true` + 补 `node-pty: true`）。

开发期手动预演（只需预演 bridge 的 add；安装层由壳登记）：

```powershell
$side = "D:\project\open-source\DSH-desk\apps\desktop\src-tauri\sidecar-dist"
$env:Path = "$side\pnpm;" + $env:Path

& "$side\node\node.exe" "$side\node_modules\@deepseek-ai\dsh\lib\bin.js" plugin --profile desktop add "@cjiaojiao/dsh-desk-bridge@0.1.0"
```

（bridge spec 缺省版本 = `apps/desktop/src-tauri/Cargo.toml` 的 `version`，经 `env!("CARGO_PKG_VERSION")` 注入；开发期可用 `DSH_DESK_BRIDGE_SPEC=link:<绝对路径>` 覆盖为本地链接。）

初始化后 `~/.dsh/profiles/desktop/package.json` 的终态（bundles 含安装层 + bridge，dependencies 只有 bridge）：

```json
{
  "name": "dsh-profile-desktop",
  "private": true,
  "dependencies": { "@cjiaojiao/dsh-desk-bridge": "0.1.0" },
  "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "@cjiaojiao/dsh-desk-bridge"] } }
}
```

## 启动（开发）

```powershell
# 方式 A：tauri dev（有 watcher）
pnpm desktop:dev

# 方式 B：直接跑构建产物（更快，无 watcher）
D:\project\open-source\DSH-desk\target\debug\dsh-desk.exe
```

开发构建会自动用源码目录 `apps/desktop/src-tauri/sidecar-dist` 作为 sidecar（无需再设 `SIDECAR_ROOT`）；仍可用 `$env:SIDECAR_ROOT = "..."` 覆盖为任意目录（冒烟/调试用）。

启动序列：解析 sidecar → 初始化 desktop profile（幂等：登记安装层 bundles `dsh-base` + `dsh-web-app`、bridge 缺则补，历史残留自动迁移）→ 选空闲端口 → 拉起 sidecar（`dsh --profile desktop --port N --no-open`，环境注入 `DSH_TELEMETRY_DISABLED=1` 与自带 pnpm 的 PATH）→ 等 stdout 的 `dsh web: http://…` 就绪行 → WebView 自动导航。崩溃按 1s/2s/4s 退避重启（3 次上限），耗尽后展示错误页（重试 / 打开日志目录 / 退出）。托盘菜单：显示/隐藏、重启宿主、打开日志目录、退出。

生产包只携带 `sidecar-dist.tar` + `sidecar-version.json`：首启把它们解压到 `%LOCALAPPDATA%\com.dsh.desk\sidecar-dist`（等待页显示进度条），按 VERSION.json 幂等，之后启动直接复用；环境变量 `DSH_DESK_SIDECAR_CACHE` 可覆盖缓存目录（冒烟/调试用）。`SIDECAR_ROOT` 仍可指向任意解压好的目录绕过上述流程。

## 测试

```powershell
cargo test -p desk-core     # 监督状态机/就绪解析/路径/日志/profile/config/credentials 单测（45 例）
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
- 安装包只含 shell + `sidecar-dist.tar` + `sidecar-version.json`（实测约 55MB 压缩后），安装即「复制 1 个大文件」，不再逐文件解压 3 万多个 node_modules 文件；
- 首启在等待页显示「正在准备本地环境… N%」解压进度（0.5–2 分钟，取决于磁盘与杀软），之后启动秒开；升级版本（`VERSION.json` 三字段任一变化）自动重解；
- 安装包免管理员（`installMode: currentUser`），WebView2 缺失时自动引导安装（`downloadBootstrapper`）；
- portable zip 组装脚本（`scripts/build-portable.mjs`）同样只携带 tar（解压更快），首启解压逻辑与安装版一致；
- v1 不签名（SmartScreen 见「常见问题」）。

## 日志

壳与 sidecar 的滚动日志（1MB×2）在 `~/.dsh/desktop/logs/`（尊重 `DSH_HOME`）：

- `shell-YYYYMMDD.log` —— 壳事件（启动、就绪、重启、失败原因）；
- `sidecar-YYYYMMDD.log` —— sidecar 原始输出。

托盘「打开日志目录」或错误页按钮可直达。

## 常见问题（开发机实测）

- **cargo 报 `SEC_E_NO_CREDENTIALS` / npm 下载极慢**：schannel 系工具（cargo/curl）在某些网络环境不可用；Node 系（pnpm/脚本）走 OpenSSL 不受影响。cargo 配 rsproxy 镜像 + 完整访问权限即可；pnpm 下载超时可在仓库根自行加 `.npmrc`（`fetch-timeout=600000` 等）放宽。
- **`link.exe not found`**：默认 MSVC 工具链缺 C++ 链接器。装 VS 2022 Build Tools（「使用 C++ 的桌面开发」工作负载）并 `rustup default stable-x86_64-pc-windows-msvc` 即可。详见 `docs/rust-install.zh.md`。
- **`dlltool.exe not found`**：切到 `x86_64-pc-windows-gnu` 后的连环坑——rustup 自带 MinGW 是精简版（缺 `as.exe`），`windows-sys` 的 raw-dylib 无法生成导入库。本项目只支持 MSVC 工具链，切回即可。详见 `docs/rust-install.zh.md`。
- **`dsh plugin add` 失败 404 `dsh-frontend`**：历史版本里 npm `latest` 曾指向废弃版本；务必显式钉版本（与 `DSH_VERSION` 一致，当前 `@0.1.1-rc.2`，见上节「初始化 desktop profile」）。
- **pnpm 报 `ERR_PNPM_IGNORED_BUILDS`**：profile 与 sidecar 的 `pnpm-workspace.yaml` 都要有 `allowBuilds`（koffi/node-pty）；koffi 的二进制经 optional 依赖 `@koromix/koffi-win32-x64` 分发，其 install 脚本检测到二进制后自动跳过编译，无需本机 C++ 工具链。
- **组装脚本不能加 `--no-optional`**：sharp/koffi 的平台二进制都走 optionalDependencies，加了会导致宿主启动失败（曾踩坑）；安装必须保留 optional（`pnpm install --prod`，allowBuilds 白名单写在 sidecar 的 `pnpm-workspace.yaml`）。
- **tauri dev 反复重建**：watcher 会把约 320MB 的 `sidecar-dist` 当源码监控；开发期建议用方式 B（直接跑 exe），watcher 忽略配置待优化。
- **与既有 dsh CLI 共存**：桌面永远用自己的 sidecar，不检测、不借用、不升级用户系统里的 dsh；共享的只是 `~/.dsh` 用户数据（profile、会话、凭证）。
- **SmartScreen（未签名）**：安装时选「更多信息 → 仍要运行」；用户版步骤与示意图见[根 README](../README.md#smartscreen-提示未签名预期行为)。

## 不变式

开源、自部署、无中心服务器；**零遥测**（sidecar 强制 `DSH_TELEMETRY_DISABLED=1`，现在不做、以后也不做）；发布唯一渠道 GitHub Releases；插件分发走公共 npm registry；模型用 DeepSeek 官方 API（用户自己的 key）；不削弱 harness 现有信任模型（沙箱/审批栈完全宿主执行，桌面只做通知镜像）。
