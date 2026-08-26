# DSH-desk

> 基于 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) + [Tauri](https://tauri.app/) 开发的 **Windows 桌面应用**。
> **极简轻量，告别庞大安装包**

## 特点与优势

- **开箱即用**：免安装 Node.js、免配置 pnpm、不用碰命令行，双击即用。
- **windows系统功能集成**：开机自启开关、快捷键展示、重启dsh、一键打开日志目录、查看最新版本；dsh审批事件会镜像为系统通知。
- **与网页版数据互通**：桌面端与网页版共享同一份用户数据（`~/.dsh` 下的 profile、会话、凭证），两边切换无缝衔接。
- **插件生态完全复用**：内置[dsh-market](https://github.com/dsh-market/dsh-market/tree/main)插件市场，沿用 Harness 的 profile 与插件机制，社区插件照常安装使用。
- **安装轻量、启动快**：sidecar 以单个归档文件随安装包分发；首次运行在本地解压并显示进度，之后每次启动秒开，升级版本自动刷新。
- **安全边界清晰**：WebView 仅放行本地内嵌页面与本机 Harness 源，外部链接一律移交系统浏览器打开；凭证只存在用户本地的 Harness 目录中，桌面壳不读取其内容。

## 系统要求

| 项目 | 要求 |
|---|---|
| 操作系统 | Windows 10 / 11，x64 |
| WebView2 Runtime | Evergreen（Windows 11 自带；Windows 10 缺失时安装器会自动引导安装） |
| 管理员权限 | 不需要（每用户安装） |

## 下载与安装

所有安装包只从 **GitHub Releases**（唯一发布渠道）下载：<https://github.com/JiaosSir/DSH-desk/releases>

### 方式一：安装版（推荐）

1. 下载 `DSH-desk_<版本>_x64-setup.exe`；
2. 双击安装（免管理员权限，可自选安装目录，缺省为当前用户的 Programs 目录）；
3. 从开始菜单或桌面快捷方式启动。

### 方式二：便携版

1. 下载 `DSH-desk-portable-<版本>-x64.zip`；
2. 解压到任意目录（建议放在固定位置，如 `D:\DSH-desk`）；
3. 双击 `dsh-desk.exe` 即可运行。

## 与 CLI 版 dsh / 网页版共存

- 桌面端与网页版**共享同一份用户数据**：`~/.dsh` 下的 profile、会话、凭证、插件。
- 桌面端**永远使用随包自带的引擎**（自带 Node + `@deepseek-ai/dsh`），独立于命令行安装的 dsh。
- 建议桌面端与你常用的 dsh CLI 版本保持**同一 rc 版本**（当前 `0.1.1-rc.2`），避免版本交错导致 profile 依赖反复重装（详见 [FAQ](docs/FAQ.md#桌面版和-cli-版-dsh-能一起用吗)）。

## 升级与卸载

- **升级**：直接下载新版安装包**覆盖安装**即可；`~/.dsh` 中的 profile、会话、凭证原样保留，升级后首次启动自动刷新引擎版本。
- **卸载**：通过「设置 → 应用 → 已安装的应用」卸载，或重跑安装包选择卸载；卸载**不会删除** `~/.dsh` 中的个人数据（如需彻底清理，手动删除该目录）。
- **便携版**：删除解压目录即完成卸载。

## 隐私

- **零遥测**：sidecar 强制 `DSH_TELEMETRY_DISABLED=1`；无统计、无上报、无广告。
- **本地日志**：在 `~/.dsh/desktop/logs/`（或 `DSH_HOME` 环境变量定义的路径），可通过托盘菜单或错误页「打开日志目录」直达；日志仅用于排查问题，可随时删除。

## 桌面特性速览

| 特性 | 说明 |
|---|---|
| 托盘菜单 | 显示/隐藏窗口、重启dsh、打开日志目录、退出 |
| 全局快捷键 | 缺省 `Ctrl+Alt+D` 唤起/隐藏（改键：编辑 `~/.dsh/desktop/config.json` 的 `hotkey`，重启应用生效；非法值自动回退缺省） |
| 系统通知 | dsh收到审批请求时镜像通知，审批操作仍在窗口内完成 |
| 开机自启 | 设置区「桌面」分区开关，默认关 |
| 窗口记忆 | 尺寸/位置自动记忆，下次启动恢复 |
| 单实例 | 二次启动自动聚焦已有窗口 |

## 相关链接

- 上游项目：[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（MIT）
- 常见问题：[`docs/FAQ.md`](docs/FAQ.md)
- 开发者文档：[`docs/README.md`](docs/README.md)（环境准备/启动/测试）与 [`docs/dev/BUILDING.md`](docs/dev/BUILDING.md)（构建与发布）
- windows系统功能桥接插件：[`packages/dsh-desk-bridge`](packages/dsh-desk-bridge)
- 开源许可：[`LICENSE`](LICENSE)（MIT）
