# DSH-desk

> 基于 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 二次开发的 **Windows 桌面应用程序**。

DSH-desk 是 DeepSeek Harness 的桌面分发形态：它以二次开发的方式，把 Harness 的 Web 界面封装为原生 Windows 桌面应用。宿主引擎（`@deepseek-ai/dsh`）、Web UI、客户端插件体系与 Slot/主题生态全部来自 DeepSeek Harness 并保持完全兼容；本项目在其之上增加了一层 Tauri 2 桌面壳与桌面级集成能力（窗口、托盘、快捷键、通知、开机自启等）。

## 特点与优势

- **开箱即用**：免安装 Node.js、免配置 pnpm、不用碰命令行——应用随包自带 Node 运行时与钉死版本的 Harness 引擎（sidecar），双击即用。
- **桌面级体验**：独立窗口、系统托盘、全局快捷键（唤起/隐藏）、开机自启、系统通知、单实例运行；关闭窗口即退出，不留后台进程。
- **深度集成而非简单套壳**：通过随包安装的桥接插件，在 Web UI 设置区新增「桌面」分区——开机自启开关、快捷键展示、重启宿主、一键打开日志目录、查看最新版本；宿主审批事件会镜像为系统通知。
- **与网页版数据互通**：桌面端与网页版共享同一份用户数据（`~/.dsh` 下的 profile、会话、凭证），两边切换无缝衔接；还支持从网页版 profile 向桌面端一键导入插件。
- **插件生态完全复用**：沿用 Harness 的 profile 与插件机制，社区插件照常安装使用。
- **安装轻量、启动快**：sidecar 以单个归档文件随安装包分发（压缩由安装器完成），安装只解一个大文件（不再逐文件解压数万个小文件）；首次运行在本地解压并显示进度，之后每次启动秒开，升级版本自动刷新。
- **安全边界清晰**：WebView 仅放行本地内嵌页面与本机 Harness 源，外部链接一律移交系统浏览器打开；凭证只存在用户本地的 Harness 目录中，桌面壳不读取其内容。

## 运行方式

### 方式一：安装版（推荐）

1. 从 [Releases](https://github.com/JiaosSir/DSH-desk/releases) 下载 `DSH-desk_<版本>_x64-setup.exe`；
2. 双击安装（免管理员权限，可自选安装目录）；
3. 从开始菜单或桌面快捷方式启动。首次运行会自动准备本地环境（解压运行引擎、初始化插件配置），等待页会显示进度条，完成后自动进入主界面。

系统要求：Windows 10/11 x64。若系统缺少 WebView2 运行时，安装器会自动引导安装。

### 方式二：便携版

1. 下载 `DSH-desk-portable-<版本>-x64.zip`；
2. 解压到任意目录，双击 `dsh-desk.exe` 即可运行，无需安装。首次运行同样会自动准备本地环境。

### 方式三：开发者运行

从源码运行需 Node 22+、pnpm 10+ 与 Rust（MSVC 工具链）：

```powershell
pnpm install                  # Node 依赖
pnpm sidecar:assemble         # 组装内嵌 Harness 引擎（sidecar）
pnpm desktop:dev              # 开发模式启动（窗口 + 热重载）
```

打包与更多开发细节见 [`docs/README.md`](docs/README.md)。

## 技术架构（简述）

```
┌────────────────────────────────────────────┐
│ Tauri 2 壳（Rust）                          │
│  窗口/托盘/快捷键/通知 · IPC 桥 · 监督任务    │
└───────────────┬────────────────────────────┘
                │ 监督 sidecar：启动 → 等待就绪 → WebView 导航
┌───────────────▼────────────────────────────┐
│ sidecar：自带 Node + pnpm + @deepseek-ai/dsh │
│ 在 127.0.0.1 起 Harness 宿主（Web UI + API）  │
└───────────────┬────────────────────────────┘
                │ window.__DSH_DESK__（桥接插件）
                ▼
        WebView2 中的 Harness Web UI
```

- **壳**：Tauri 2（Rust），不含业务前端，只承载等待页/错误页与桌面能力；
- **宿主**：DeepSeek Harness 以 sidecar 方式随包分发，自备 Node 与钉版依赖；
- **桥接**：npm 包 [`@cjiaojiao/dsh-desk-bridge`](packages/dsh-desk-bridge) 提供宿主半部（事件旁听 + `/api/desktop/*` 路由）与浏览器半部（设置区 + 特性检测），在纯浏览器中自动退化为空操作。

## 相关链接

- 上游项目：[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
- 开发文档：[`docs/README.md`](docs/README.md)
- 桥接插件：[`packages/dsh-desk-bridge`](packages/dsh-desk-bridge)
