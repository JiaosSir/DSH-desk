# 安装 Rust（cargo）

## 1. 下载并安装

打开 [https://rustup.rs](https://rustup.rs)，点「下载」，运行 `rustup-init.exe`。

安装过程一路回车用默认选项即可（选默认工具链 `stable`）。

## 2. 国内网络加速（可选但推荐）

网络慢的话，用镜像安装。先在 PowerShell 里执行：

```powershell
$env:RUSTUP_DIST_SERVER = "https://rsproxy.cn"
$env:RUSTUP_UPDATE_ROOT = "https://rsproxy.cn/rustup"
rustup-init.exe -y
```

## 3. 让 cargo 命令随处可用

```powershell
[Environment]::SetEnvironmentVariable('Path', "$env:USERPROFILE\.cargo\bin;" + [Environment]::GetEnvironmentVariable('Path','User'), 'User')
```

**新开一个 PowerShell 窗口**，验证：

```powershell
cargo --version
```

## 4. 补充常用组件

```powershell
rustup component add rustfmt clippy
```

## 5. 配置依赖镜像（可选）

在 `C:\Users\你的用户名\.cargo\config.toml` 写入：

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
```

## 6. 工具链与 C++ 链接器（Windows 必读）

desk-core 依赖 `windows-sys` / `getrandom`，Windows 上编译它们需要 **MSVC 工具链 + `link.exe`**。务必使用默认的 MSVC 工具链，不要切换成 GNU 工具链（见坑 2）。

### 坑 1：`link.exe not found`

```
error: linker `link.exe` not found
note: the msvc targets depend on the msvc linker ...
```

原因：默认工具链是 `stable-x86_64-pc-windows-msvc`，但机器上没装 VS C++ 工具——`link.exe` 由 VS Build Tools 的「使用 C++ 的桌面开发」工作负载提供。装过 VS 但没勾该工作负载、或 VS 安装不完整 / 已被清理，都会触发（`vswhere` 里可能残留「Visual Studio」注册信息，但实际的 `VC\Tools\MSVC\...\link.exe` 文件并不存在）。

解决 —— 安装 MSVC Build Tools：

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --source winget --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
```

> 若 winget 报 `搜索源时失败: msstore` / 证书不匹配，是 `msstore` 源坏了，加 `--source winget`（如上）即可绕过。
> 也可图形界面安装：下载 <https://visualstudio.microsoft.com/visual-cpp-build-tools/> ，勾选「使用 C++ 的桌面开发」工作负载。

### 坑 2：`dlltool.exe not found`（切勿切换 GNU 工具链）

切到 `stable-x86_64-pc-windows-gnu` 后会连环报错：

```
error: error calling dlltool 'dlltool.exe': program not found
error: could not compile `windows-sys` (lib) due to 1 previous error
```

原因：`windows-sys` 走 `raw-dylib`，rustc 需要 `dlltool.exe` 生成导入库，但只按裸名字走 PATH 查找；而 rustup 自带的 MinGW 是精简版（只有 `gcc`/`ld`/`dlltool`，缺 `as.exe`），即便把它的 `self-contained` 目录加进 PATH，`dlltool` 内部仍会因找不到 `as.exe` 而 `CreateProcess` 失败。这是 rustc 的已知缺陷（rust-lang/rust#140704、getrandom#723）。

结论：**本项目只支持 MSVC 工具链**。若误切了 GNU，切回来即可：

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

### 完整步骤（新机器一次到位）

```powershell
# 1. 装 MSVC C++ Build Tools（含 link.exe）
winget install Microsoft.VisualStudio.2022.BuildTools --source winget --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"

# 2. 确保默认工具链是 MSVC
rustup default stable-x86_64-pc-windows-msvc

# 3. 重开终端，验证
cargo test -p desk-core
```

## 验证

```powershell
cd .\DSH-desk
cargo test -p desk-core
```

出现 `test result: ok. 37 passed` 即安装成功。