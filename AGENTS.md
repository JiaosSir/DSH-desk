# DSH-desk

基于 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) + [Tauri 2](https://tauri.app/) 的 **Windows 桌面应用**（仅 Windows 10/11 x64，依赖 WebView2）。Tauri（Rust）壳负责系统集成（托盘、全局快捷键、通知、自启、单实例）并监督 sidecar 生命周期；sidecar 是随包分发的自带引擎（Node + pnpm + `@deepseek-ai/dsh`，免安装 Node/pnpm）。用户数据位于 `~/.dsh`，与网页版 / CLI 版 dsh 共享。

仓库是双工作区 monorepo：pnpm workspace（`apps/*`、`packages/*`）+ Cargo workspace（`crates/desk-core`、`apps/desktop/src-tauri`）。文档以中文为主。

## Build

前置要求（仅 Windows）：

- Rust stable 工具链（rustup，MSVC）+ WebView2 Runtime
- Node.js 22+ 与 pnpm 11（`package.json` 钉 `packageManager: pnpm@11.7.0`）
- 可访问网络以下载 sidecar 资产（下载缓存目录 `.downloads/`，命中即免下载）

构建步骤（按顺序）：

```bash
# 1. 安装依赖（esbuild 的 postinstall 已显式关闭，见 pnpm-workspace.yaml）
pnpm install

# 2. 构建 bridge 包（tsc + tsdown，产物 packages/dsh-desk-bridge/lib/）
pnpm bridge:build

# 3. 组装 sidecar（幂等；版本钉在 scripts/assemble-sidecar.mjs 顶部常量：
#    NODE_VERSION=24.19.0 / PNPM_VERSION=11.7.0 / DSH_VERSION=0.1.1-rc.2；
#    --force 强制重建；产物 apps/desktop/src-tauri/sidecar-dist/ + sidecar-dist.tar + sidecar-version.json）
pnpm sidecar:assemble

# 4. 编译桌面壳（release exe → target/release/dsh-desk.exe）
pnpm --dir apps/desktop tauri build --no-bundle

# 5. 安装包（NSIS → target/release/bundle/nsis/*-setup.exe）与便携版（dist-portable/*.zip）
pnpm desktop:build
pnpm desktop:portable
```

要点：

- WebView 前端是**提交在仓库里的静态页** `apps/desktop/dist/`（`index.html`、`error.html`），无前端框架构建步骤；bridge 注入由 dsh 宿主机制完成，不要在这里另起前端构建。
- 所有生成物（`sidecar-dist/`、`*.tar`、`lib/`、`target/`、`dist-portable/`）均在 `.gitignore` 中且可复现，**不要手改、不要提交**。
- CI 中的等价命令见 `.github/workflows/ci.yml`（`shell-check` job）。

## Test

```bash
# Rust 核心库（sidecar 监督、端口协商、profile 初始化、滚动日志；CI 强制 fmt/clippy）
cargo fmt -p desk-core --check
cargo clippy -p desk-core -- -D warnings
cargo test -p desk-core

# bridge 包（vitest）
pnpm bridge:test

# 仓库脚本纯函数（node --test：assemble-sidecar / build-portable / zip-entries）
pnpm test:scripts

# 端到端冒烟：先构建 release exe，再 spawn 它并注入
# DSH_DESK_SMOKE=1、DSH_HOME=<临时目录>、DSH_DESK_SIDECAR_CACHE=<临时目录>，
# 断言 exit 0 且输出含 SMOKE_OK（失败时打印 profile 日志尾部）
pnpm --dir apps/desktop tauri build --no-bundle
pnpm smoke
```

## Run

```bash
# 开发运行（tauri dev；首次需先执行 pnpm sidecar:assemble）
pnpm desktop:dev

# 生产 exe 直接运行：target/release/dsh-desk.exe
# 首次运行从 exe 旁 sidecar-dist.tar 解压 sidecar 到缓存目录并显示进度，之后秒开
```

运行时事实：

- **数据**：`~/.dsh`（profile、会话、凭证、插件，与网页版 / CLI 共享）；可用 `DSH_HOME` 覆盖根目录。
- **配置**：`~/.dsh/desktop/config.json`（如全局快捷键 `hotkey`，缺省 `Ctrl+Alt+D`；非法值自动回退缺省）。
- **日志**：`~/.dsh/desktop/logs/`（sidecar-* / shell-*），托盘菜单或错误页可直达。
- **安全边界**：WebView 仅放行本地内嵌页面与本机 Harness 源，外部链接一律交系统浏览器；sidecar 强制 `DSH_TELEMETRY_DISABLED=1`（零遥测）。
- 单实例（二次启动聚焦已有窗口）、托盘、自启开关、窗口尺寸/位置记忆。

## Workflow

- **分层**：`crates/desk-core` 是不依赖 tauri 的核心库（sidecar 监督、端口协商、profile 初始化、滚动日志），`apps/desktop/src-tauri` 是 Tauri 壳（`lib.rs`、`commands.rs`、`tray.rs`、`shortcuts.rs`、`bridge.rs`、`smoke.rs`），`packages/dsh-desk-bridge` 是发布到 npm 的 `@cjiaojiao/dsh-desk-bridge`（浏览器环境自动降级为 no-op，独立版本号、不随 tag 派生）。
- **发布流程**：打 tag `v*` → `.github/workflows/release.yml`（发布 bridge → 由 tag 改写 `tauri.conf.json` 与壳包版本 → `sidecar:assemble` → `tauri build --bundles nsis` → `build-portable` → 冒烟 → GitHub Release）。
- **版本钉版**：sidecar 三版本只改 `scripts/assemble-sidecar.mjs` 顶部常量；bridge 的 dsh 依赖钉 `0.1.1-rc.2`（rc 包在 `pnpm-workspace.yaml` 的 `minimumReleaseAgeExclude` 中豁免，升级需同步更新两处）。
- **改动边界**：不改 `.gitignore` 中的生成物；`desk-core` 不得引入 tauri 依赖；桌面端永远使用随包自带引擎，不依赖系统 Node/pnpm。
- **验证**：任何涉及 Rust 壳或 desk-core 的改动，提交前跑 `cargo fmt/clippy/test -p desk-core` 与 `pnpm smoke`；涉及脚本的改动跑 `pnpm test:scripts`；涉及 bridge 的改动跑 `pnpm bridge:test`。
