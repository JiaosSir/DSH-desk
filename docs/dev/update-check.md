# 检查更新功能开发调试

本文档面向开发/调试，说明检查更新链路（启动自动检查、设置区手动检查、侧边栏下载横幅）相关的环境变量与测试方法。正常使用无需设置任何环境变量。

## 环境变量

| 变量 | 取值 | 作用 |
|---|---|---|
| `DSH_DESK_UPDATE_MODE` | `installed` / `portable` | 强制按安装版 / 便携版处理更新逻辑。默认按 exe 同目录是否存在 `uninstall.exe` 自动判定（NSIS 安装版会写，便携版没有） |
| `DSH_DESK_UPDATE_CURRENT_VERSION` | 任意版本号，如 `0.9.0` | 强制参与比较与展示的「当前版本」。填一个低于 GitHub 最新 release 的版本号即可触发「有新版本」链路，不必改 `tauri.conf.json`；空值/未设置回退应用真实版本 |

相关但不属本轮新增的变量：

| 变量 | 取值 | 作用 |
|---|---|---|
| `DSH_DESK_BRIDGE_SPEC` | npm spec 或 `file:` 本地 tarball | 覆盖 bridge 插件的安装 spec（壳默认装 `@cjiaojiao/dsh-desk-bridge@<钉版>`）；dev 预览未发布的 bridge UI 用 |
| `DSH_DESK_SIDECAR_CACHE` | 目录路径 | 覆盖 sidecar 解压缓存目录（默认 `%LOCALAPPDATA%\com.dsh.desk\sidecar-dist`） |

## 测试方法

### 触发「有新版本」（侧边栏横幅 / 设置区对比）

dev 环境默认被判定为便携版（`target/debug` 下没有 `uninstall.exe`），启动自动检查会直接跳过；且本地版本与 GitHub 最新 release 相等时不会提示。组合两个变量即可全链路验证：

```powershell
$env:DSH_DESK_UPDATE_MODE = "installed"              # 强制安装版链路
$env:DSH_DESK_UPDATE_CURRENT_VERSION = "0.9.0"       # 低于 GitHub 最新 release 即触发
pnpm desktop:dev
```

预期：启动后侧边栏 logo 区与「新建会话」之间出现「下载更新 v<最新版>」横幅；设置区手动「检查更新」显示「发现新版本 <最新版>（当前 0.9.0）」。

### 强制便携版行为

在已安装的环境里模拟便携版（验证便携版不弹横幅、只给手动提示）：

```powershell
$env:DSH_DESK_UPDATE_MODE = "portable"
```

### 预览未发布的 bridge UI

```powershell
pnpm --dir packages/dsh-desk-bridge build
pnpm --dir packages/dsh-desk-bridge pack            # 生成 *.tgz
$env:DSH_DESK_BRIDGE_SPEC = "file:C:/绝对路径/xxx.tgz"
pnpm desktop:dev
```

profile 里已装 bridge 版本与钉版不一致时，壳会按 `DSH_DESK_BRIDGE_SPEC`（或默认钉版）自动升级，无需手动删 profile 条目。

## 注意事项

- `available` 判定为**严格大于**：GitHub 最新 release 必须高于当前版本才提示；相等显示「已是最新版本」，不提示降级。
- GitHub `releases/latest` 只返回**非 draft、非 prerelease** 的最新 release——rc 版本不会被当作「最新」。
- tag 必须是合法 semver（如 `v1.0.1`）；`v1.0`、`release-1` 这类 tag 会导致检查报「无法解析版本号」。
- 匿名限流 60 次/小时/IP：启动自动检查 + 手动点击各算一次，正常使用远低于上限。
- 检查失败（网络不可达等）在启动自动检查时**静默**（不打扰首启）；设置区手动检查会展示错误信息（含 WinHTTP 错误码）。
- 安装版安装包资产名以 `-setup.exe` 结尾，便携版以 `DSH-desk-portable-*-x64.zip` 结尾——资产不匹配时 `assetUrl` 为空，设置区下载按钮禁用。

## 已知过渡期现象

**新 bridge UI + 旧壳**（如 profile 里 bridge 已是新版、但安装的应用还是 v1.0.0 旧发布）：设置区会出现「检查更新」按钮，点击报 `bridge.checkUpdate is not a function`。原因：按钮 UI 来自 profile 的 bridge 包，而 `window.__DSH_DESK__` 桥方法由壳注入，旧壳没有 `checkUpdate` 等方法。正式发布新版应用后自然消失；本地验证请装本地构建的新安装包（`pnpm desktop:build` 产物），不要装 GitHub 上的旧版 setup.exe。
