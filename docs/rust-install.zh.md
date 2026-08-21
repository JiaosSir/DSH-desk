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

## 验证

```powershell
cd .\DSH-desk
cargo test -p desk-core
```

出现 `test result: ok. 17 passed` 即安装成功。