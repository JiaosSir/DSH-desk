# DSH-desk 构建与发布（开发者文档）

> 面向开发者：从源码构建、测试、发布 DSH-desk。
> 环境准备与日常开发（装工具链、`pnpm desktop:dev`、sidecar 组装细节）见 [`docs/README.md`](../README.md)；本文聚焦**决策依据、版本号策略、发布流程**。
> 设计规格：[`docs/specs/2026-08-19-desktop-shell-design.md`](../specs/2026-08-19-desktop-shell-design.md)；实施计划：[`docs/plans/2026-08-19-implementation-plan.md`](../plans/2026-08-19-implementation-plan.md)。

## 1. 关键实现决策摘要（计划 D1–D12 与落地形态）

| ID | 决策 | 落地形态（以代码为准） |
|---|---|---|
| D1 | sidecar 随 `bundle.resources` 分发 | **方案 A**：sidecar 组装后打成单个未压缩归档 `apps/desktop/src-tauri/sidecar-dist.tar` + 版本文件 `sidecar-version.json`（见 `scripts/assemble-sidecar.mjs` 的 `sidecarAssetPaths`）；release 首启解压到 `%LOCALAPPDATA%\com.dsh.desk\sidecar-dist`（`DSH_DESK_SIDECAR_CACHE` 可覆盖），按 `VERSION.json` 幂等；dev 模式直接用 `sidecar-dist/` 目录（`SIDECAR_ROOT` 可覆盖） |
| D2 | sidecar 强制零遥测 | 子进程 env 恒注入 `DSH_TELEMETRY_DISABLED=1`（`crates/desk-core` 与壳侧 smoke 共用） |
| D3 | 启动命令 = 自带 node 直跑 `lib/bin.js` | `<sidecar>/node/node.exe <sidecar>/node_modules/@deepseek-ai/dsh/lib/bin.js --profile desktop --port N --no-open`，cwd = profile 目录，PATH 头部注入 `<sidecar>/pnpm`（F5 的 .cmd shim 语义） |
| D4 | 日志落 `~/.dsh/desktop/logs/` | `shell-YYYYMMDD.log` / `sidecar-YYYYMMDD.log`，各 1MB×2 滚动（`crates/desk-core/src/logs.rs`）；托盘与错误页均可「打开日志目录」 |
| D5 | 等待页/错误页 = 壳内置静态资产 | `apps/desktop/dist/{index.html,error.html}` 作 `frontendDist`；就绪后由资产页自身 `location.replace(url)` 切到 sidecar（替换历史条目，返回键不回退等待页；桥接脚本上报页面类别，壳仅在页面已离开资产页/桥不可用时 `webview.navigate` 兜底） |
| D6 | 所有 dsh 命令由壳执行，bridge 零业务逻辑 | `apps/desktop/src-tauri/src/commands.rs` 全量命令；bridge 只做特性检测 + IPC（`window.__DSH_DESK__`） |
| D7 | bridge 独立版本、首启钉版安装 | 包名实际为 **`@cjiaojiao/dsh-desk-bridge`**；壳首启经 `dsh plugin --profile desktop add @cjiaojiao/dsh-desk-bridge@<版本>` 安装（`<版本>` = `profile::BRIDGE_PACKAGE_VERSION`，即 `packages/dsh-desk-bridge/package.json` 的 `version`，与壳版本解耦）；开发期 `DSH_DESK_BRIDGE_SPEC=link:<绝对路径>` 覆盖为本地链接 |
| D8 | 首次引导在页面内复用 web onboarding | API key 走 harness 现有机制（`~/.dsh/.env` / `.credentials.yaml`），壳只做存在性检测（`crates/desk-core/src/credentials.rs`，不读值）；**workspace 特性已移除**（`config.json` 现仅 `hotkey` / `autostart` 两字段） |
| D9 | CI 冒烟 = `DSH_DESK_SMOKE=1` 无窗口自测 | 壳 `smoke.rs`：初始化 profile → 拉起 sidecar → 等就绪 → `GET /` 断言 200 与页面标题 → `SMOKE_OK`；驱动脚本 `scripts/smoke-desktop.mjs` |
| D10 | 钉版常量集中一处 | `scripts/assemble-sidecar.mjs` 顶部：`NODE_VERSION` / `PNPM_VERSION` / `DSH_VERSION`（**升级点，见 §3**） |
| D11 | 快捷键 v1 从 config.json 读、只读展示 | 缺省 `Ctrl+Alt+D`；非法值回退缺省（`apps/desktop/src-tauri/src/shortcuts.rs`） |
| D12 | NSIS 每用户 + portable zip；v1 不签名 | `tauri.conf.json`：`installMode: currentUser`、`webviewInstallMode: downloadBootstrapper`；便携包由 `scripts/build-portable.mjs` 组装 |

## 2. 仓库地图

```
apps/desktop/                # Tauri 2 壳（Rust + 等待/错误页静态资产）
  src-tauri/                 #   main/lib/commands/bridge/tray/shortcuts/smoke.rs
  src-tauri/sidecar-dist/    #   sidecar 产物（生成物，不入库；tar 资产同目录）
  src-tauri/sidecar-dist.tar #   release 打包资产（生成物，不入库）
crates/desk-core/            # 纯逻辑 crate：监督状态机、端口、就绪解析、路径、日志、profile、config、credentials
packages/dsh-desk-bridge/    # 桥接插件（npm 发布：宿主半部 lib/index.js + 浏览器半部 lib/client.js）
scripts/                     # assemble-sidecar.mjs / smoke-desktop.mjs / build-portable.mjs / zip-entries.mjs
docs/                        # README（用户）/ FAQ / dev/BUILDING（本文）/ specs / plans
.github/workflows/           # ci.yml（PR）+ release.yml（tag v*）
```

## 3. sidecar 组装与版本升级点（D10）

```powershell
pnpm sidecar:assemble            # 幂等：VERSION.json 三版本一致则跳过下载/安装
node scripts/assemble-sidecar.mjs --force   # 强制重建
```

**升级 harness / Node / pnpm = 修改 `scripts/assemble-sidecar.mjs` 顶部常量**（唯一升级点）：

```js
export const NODE_VERSION = '24.19.0'    // Node LTS（harness engines >=24.0.0）
export const PNPM_VERSION = '11.7.0'     // pnpm ≥10（profile 的 pnpm-workspace.yaml 语义）
export const DSH_VERSION = '0.1.1-rc.2'  // 钉死 @deepseek-ai/dsh（npm 最新 rc）
```

升级步骤：

1. 改 `DSH_VERSION`（其余常量按需）→ `node scripts/assemble-sidecar.mjs --force`；
2. bridge 包的 `@deepseek-ai/dsh-*` devDeps 与 `DSH_VERSION` **同表升级**（`packages/dsh-desk-bridge/package.json`）；
3. 本地冒烟 + 手工验证（`pnpm desktop:dev` 或 release 产物）；
4. 发新版（§5）。注意：desktop profile 里 `dsh-base` / `dsh-web-app` 是安装层 bundles（**只登记包名，版本由 sidecar 解析**，壳从不 `pnpm add` 它们），升级 harness 只需重新组装 sidecar，profile 无需重建；若历史 profile 的 `dependencies` 残留 web-app 或废弃包名 `@JiaosSir/dsh-desk-bridge`，壳首启会自动迁移清理（移出 dependencies 并重建 node_modules）。

下载缓存在 `.downloads/`（手动放入 `node-v*-win-x64.zip`、`pnpm-*.tgz` 可加速）。组装脚本单测：`pnpm test:scripts`（全离线）。

## 4. 本地构建与测试

```powershell
pnpm install                       # Node 依赖（bridge、tauri CLI）
cargo test -p desk-core            # 监督状态机/就绪解析/路径/日志/profile/config/credentials 单测
cargo clippy -p desk-core -p dsh-desk --all-targets -- -D warnings
pnpm bridge:test                   # 桥接插件退化契约（vitest + jsdom）
pnpm bridge:build                  # 产物：lib/index.js（宿主）+ lib/client.js（浏览器 IIFE）

pnpm sidecar:assemble              # 组装 sidecar（release 打包前必做）
pnpm --dir apps/desktop tauri build --bundles nsis   # NSIS 安装包
node scripts/build-portable.mjs    # portable zip（从 target/release 收集 exe + tar 资产）
node scripts/smoke-desktop.mjs     # 冒烟（默认 target/release/dsh-desk.exe）
```

**冒烟测试用法**（D9）：

```powershell
node scripts/smoke-desktop.mjs [exe 路径]   # 默认 target/release/dsh-desk.exe
```

- 脚本 spawn exe，注入 `DSH_DESK_SMOKE=1`、`DSH_HOME=<mkdtemp>`、`DSH_DESK_SIDECAR_CACHE=<mkdtemp>`，5 分钟超时；
- 断言退出码 0 且输出含 `SMOKE_OK`；失败打印 profile 日志尾部；
- release 产物走**生产链路**（从 exe 旁 `sidecar-dist.tar` 解压到临时缓存再起宿主），不设 `SIDECAR_ROOT`；
- 开发期排错：`SIDECAR_ROOT=<目录>` 可让壳直接使用任意解压好的 sidecar 目录（冒烟/调试用）。

开发期把 bridge 装进桌面 profile（D7 覆盖）：

```powershell
$env:DSH_DESK_BRIDGE_SPEC = "link:D:\project\open-source\DSH-desk\packages\dsh-desk-bridge"
pnpm desktop:dev
```

## 5. 发布流程（release.yml）

**版本号策略**：壳与 sidecar 共用版本号（由 git tag 派生）；bridge 独立版本（`packages/dsh-desk-bridge/package.json` 的 `version`，壳侧经 `profile::BRIDGE_PACKAGE_VERSION` 读取，与壳版本解耦）。release.yml 自动写入 `tauri.conf.json`（安装包版本）与各 npm workspace 包版本（bridge 发布版本）；**`apps/desktop/src-tauri/Cargo.toml` 的 `version` 必须手工同步为 tag 版本**。仓库基线：壳 `1.0.0`、bridge `0.1.0`。

发布步骤：

```powershell
git tag v0.2.0            # 例：版本号与 tag 同号
git push origin v0.2.0
```

`release.yml` 两个 job（`publish-bridge` 成功后才跑 `build-release`）：

1. **publish-bridge**（ubuntu）：`pnpm bridge:test` → 桥包 version 对齐 tag（`npm pkg set version=${GITHUB_REF_NAME#v}`）→ `pnpm bridge:build` → `pnpm --filter @cjiaojiao/dsh-desk-bridge publish --access public --no-git-checks`（需要仓库 secrets 配置 `NPM_TOKEN`，仅此 job 使用）。
2. **build-release**（windows，`needs: publish-bridge`）：版本号由 tag 派生写入 `tauri.conf.json` 与各 workspace 包 → `pnpm sidecar:assemble`（**不含 bridge**，bridge 由壳首启经 npm 钉版装入 profile）→ `pnpm --dir apps/desktop tauri build --bundles nsis` → `node scripts/build-portable.mjs` → `node scripts/smoke-desktop.mjs` → `softprops/action-gh-release` 上传 `*-setup.exe` 与 `dist-portable/*.zip`。

**发布前清单**：

- [ ] `DSH_VERSION` 已升级并重新组装（§3），bridge devDeps 同表升级；
- [ ] 本地 `pnpm sidecar:assemble` + `tauri build --no-bundle` + `node scripts/smoke-desktop.mjs` 全绿；
- [ ] CI（`ci.yml`：desk-core / bridge / shell-check 三 job）绿；
- [ ] `NPM_TOKEN` secret 已配置（只读权限发布 `@cjiaojiao/dsh-desk-bridge`）。

## 6. CI 骨架（ci.yml）

| Job | 平台 | 内容 |
|---|---|---|
| desk-core | windows-latest | `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test` |
| bridge | ubuntu-latest | `pnpm bridge:test` / `pnpm bridge:build` |
| shell-check | windows-latest | `pnpm sidecar:assemble`（`.downloads` 缓存）→ `tauri build --no-bundle` → `node scripts/smoke-desktop.mjs` |

## 7. 相关指针

- 用户文档：[根 README](../../README.md)、[FAQ](../FAQ.md)
- 环境准备/日常开发（工具链、`pnpm desktop:dev`、开发 FAQ）：[`docs/README.md`](../README.md)
- 设计规格（§1 不变式、§7 原生能力、§8 首次引导、§9 分发）：[`docs/specs/2026-08-19-desktop-shell-design.md`](../specs/2026-08-19-desktop-shell-design.md)
- 实施计划（6 阶段逐条任务与验收）：[`docs/plans/2026-08-19-implementation-plan.md`](../plans/2026-08-19-implementation-plan.md)
