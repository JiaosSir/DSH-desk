# 继续 dsh-desktop：DeepSeek Harness Web UI 的 Windows 桌面分发

你接手一个已定案的头脑风暴项目。先做两件事：完整阅读 `D:\project\open-source\dsh-desktop\docs\specs\2026-08-19-desktop-shell-design.md`（唯一权威设计文档），并把 `D:\project\open-source\deepseek-harness` 当只读参考库（v1 不修改它）。

## 项目背景

- 目标：把 DeepSeek Harness Web UI 做成面向普通用户的 Windows 桌面产品——用户不装 Node、不用命令行，双击即用。
- 本仓库：`D:\project\open-source\dsh-desktop`（已 git init，首个 commit `178f20e` 含设计规格）。
- 上游：`D:\project\open-source\deepseek-harness`（npm 包 `@deepseek-ai/dsh-*` 的发布源；本机可直接 `pnpm dsh web` 起一个真实 GUI 做行为参照，需要 DEEPSEEK_API_KEY）。

## 已定决策（不要重新讨论，除非发现无法实现的硬伤）

1. 路线：Tauri 2 壳 + 系统 WebView2，harness 以 sidecar 随包分发（自带 Node + pnpm standalone + 钉死版本的 `@deepseek-ai/dsh-cli`、`dsh-web-app`、`dsh-web-frontend` dist）。壳不含前端，WebView 直接加载 sidecar 的 http://127.0.0.1:<port>，现有 Web UI、客户端插件、Slot/主题生态 100% 复用。
2. v1 零上游改动：不改 deepseek-harness 仓库任何代码。
3. 端口协商：壳选空闲端口传 `--port N`；就绪信号 = sidecar stdout 的 `dsh web: http://…` 行。绑定失败换端口重试。
4. Profile：独立 `~/.dsh/profiles/desktop/`（尊重 DSH_HOME）。首次运行用自带 node+pnpm 执行 `dsh plugin --profile desktop add @deepseek-ai/dsh-web-app` 和 `dsh plugin --profile desktop add @JiaosSir/dsh-desktop-bridge`（包名可调整）。设置内提供 web→desktop 单向插件同步。
5. 桥接插件 `packages/dsh-desktop-bridge`：npm 发布的外部插件（参照 dsh-web-ui 模式：`dsh.bundle` manifest + client bundle 经 `__DSH_BOOT__` 图加载）；只做特性检测 + Tauri IPC 桥（`window.__DSH_DESKTOP__`），业务逻辑零；浏览器里自动退化。写它之前加载 `cordis-plugin-development` skill 并研究 harness 的外部插件规范（docs/cookbook、dsh-web-ui 仓库结构）。
6. 永久不变式：开源、自部署、无中心服务器；零遥测（现在不做、以后也不做）；发布唯一渠道 = GitHub Releases；插件分发 = 公共 npm registry；模型 = DeepSeek 官方 API（用户自己的 key）。
7. 无自动更新：v1 无 updater、无任何更新请求；设置按钮文案「查看最新版」= 打开 GitHub Releases 页面。升级 = 覆盖安装，`~/.dsh` 用户数据保留。
8. 安全：sidecar 只绑 127.0.0.1 + 随机端口，trusted-host 栅栏原样；沙箱/审批栈完全宿主执行，桌面只做通知镜像；凭证走 `~/.dsh` .env；WebView 禁外部导航、外链交系统浏览器。
9. 与已装 dsh 共存：桌面永远用自己的 sidecar，不装全局 dsh、不借用用户 dsh；共享的只是 `~/.dsh` 用户数据。
10. 原生能力 v1 清单：托盘（显示/隐藏/重启宿主/退出）、通知镜像、全局快捷键 Ctrl+Alt+D 唤起/隐藏、单实例、原生文件夹选择桥、开机自启（默认关）、窗口状态记忆、本地滚动日志 + 一键打开目录。
11. 分发：NSIS 每用户安装 + portable zip；x64；v1 不签名（README 写 SmartScreen 步骤）；首发 zh-CN。
12. 仓库布局、首次引导流程、风险与验证策略：见规格文档 §7-11（原生能力表、引导 8 步、组件清单、冒烟测试策略）。

## 你的任务

1. 加载 `writing-plans` skill，基于规格文档产出一份分阶段实施计划（每阶段：目标、产出物、验收标准、风险），写入本仓库（如 `docs/plans/2026-08-19-implementation-plan.md`），git 提交。
2. 把阶段划分给用户确认，然后按计划逐阶段实现，每阶段完成即提交。建议的阶段切分参考：① 仓库脚手架（Tauri 2 app + crates + bridge 包骨架 + CI）→ ② sidecar 组装脚本与监督（含端口协商/重试/日志）→ ③ profile 初始化与首次引导 → ④ 桥接插件（原生能力 + 特性检测退化）→ ⑤ 打包与发布（NSIS + zip + GitHub Releases workflow）→ ⑥ 文档与 FAQ。
3. 遵守头脑风暴定案；任何与规格文档冲突的实现需求先与用户确认再改。

## 环境与沙箱须知

- 本会话工作区可能只是 deepseek-harness；写 `D:\project\open-source\dsh-desktop` 会被沙箱拦截。被拦后用 `sandbox_permissions: danger-full-access` + 一句理由重试，审批弹窗用户会批准；不要绕过、不要重复申请已拒绝的操作。
- 真实运行验证：`cd D:\project\open-source\deepseek-harness && pnpm dsh web`（有 DEEPSEEK_API_KEY 时）可观察真实 Web UI 行为；桌面壳侧用冒烟测试（启动壳→健康检查→页面标题→退出）。
- git 身份在 dsh-desktop 仓库里已配置为 dsh-desktop/dev@dsh-desktop.local，如需署名自行调整。

## 你第一轮应该输出的东西

- 实施计划文档路径 + 阶段划分摘要（含每阶段验收标准），等用户确认后开工。
