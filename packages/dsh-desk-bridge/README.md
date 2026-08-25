# @cjiaojiao/dsh-desk-bridge

dsh 桌面壳的桥接插件（外部插件，npm 分发）。宿主半部在 dsh 宿主进程中旁听事件并提供 `/api/desktop/*` 路由；浏览器半部只做**特性检测 + Tauri IPC 桥**（`window.__DSH_DESK__`），业务逻辑零。在纯浏览器中加载同一份 UI 时自动退化为空操作。

## 安装

```bash
dsh plugin --profile desktop add @cjiaojiao/dsh-desk-bridge
```

开发期（本地链接）：

```bash
dsh plugin --profile desktop add link:<本仓库>/packages/dsh-desk-bridge
```

## 构建与测试

```bash
pnpm install
pnpm test          # vitest（jsdom）
pnpm build         # tsc + tsdown：lib/index.js（宿主）+ lib/client.js（浏览器）
```

浏览器 bundle（`lib/client.js`）经 `window.__ModuleLoader__.load` 注册进 `__DSH_BOOT__` 模块图，与 dsh-web-ui 的外部插件同构。
