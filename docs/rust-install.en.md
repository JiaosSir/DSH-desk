# Installing Rust (cargo)
Engliish|[中文](rust-install.md)

## 1. Download and install

Open [https://rustup.rs](https://rustup.rs), click "Download", and run `rustup-init.exe`.

Press Enter through the installer with the defaults (the default `stable` toolchain is fine).

## 2. Network mirror for mainland China (optional but recommended)

If the network is slow, install via a mirror. First run in PowerShell:

```powershell
$env:RUSTUP_DIST_SERVER = "https://rsproxy.cn"
$env:RUSTUP_UPDATE_ROOT = "https://rsproxy.cn/rustup"
rustup-init.exe -y
```

## 3. Make cargo available everywhere

```powershell
[Environment]::SetEnvironmentVariable('Path', "$env:USERPROFILE\.cargo\bin;" + [Environment]::GetEnvironmentVariable('Path','User'), 'User')
```

**Open a new PowerShell window** and verify:

```powershell
cargo --version
```

## 4. Add common components

```powershell
rustup component add rustfmt clippy
```

## 5. Configure a dependency mirror (optional)

Write the following to `C:\Users\<your username>\.cargo\config.toml`:

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
```

## 6. Toolchain and C++ linker (required reading on Windows)

desk-core depends on `windows-sys` / `getrandom`; compiling them on Windows requires the **MSVC toolchain with `link.exe`**. Always use the default MSVC toolchain — do not switch to the GNU toolchain (see Pitfall 2).

### Pitfall 1: `link.exe not found`

```
error: linker `link.exe` not found
note: the msvc targets depend on the msvc linker ...
```

Cause: the default toolchain is `stable-x86_64-pc-windows-msvc`, but the machine has no VS C++ tools — `link.exe` is provided by the "Desktop development with C++" workload of VS Build Tools. This also happens if you installed VS without that workload, or the VS install is incomplete/cleaned up (the `vswhere` registry entries may linger while the actual `VC\Tools\MSVC\...\link.exe` file is gone).

Fix — install the MSVC Build Tools:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --source winget --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
```

> If winget reports `failed to search source: msstore` / certificate mismatch, the `msstore` source is broken; add `--source winget` (as above) to bypass it.
> GUI alternative: download <https://visualstudio.microsoft.com/visual-cpp-build-tools/> and select the "Desktop development with C++" workload.

### Pitfall 2: `dlltool.exe not found` (never switch to the GNU toolchain)

Switching to `stable-x86_64-pc-windows-gnu` triggers a cascade of errors:

```
error: error calling dlltool 'dlltool.exe': program not found
error: could not compile `windows-sys` (lib) due to 1 previous error
```

Cause: `windows-sys` uses `raw-dylib`, so rustc needs `dlltool.exe` to generate import libraries, but it only searches PATH by bare name. rustup's bundled MinGW is a slim build (only `gcc`/`ld`/`dlltool`, missing `as.exe`); even adding its `self-contained` directory to PATH, `dlltool` still fails with `CreateProcess` because it cannot find `as.exe`. This is a known rustc deficiency (rust-lang/rust#140704, getrandom#723).

Conclusion: **this project only supports the MSVC toolchain**. If you accidentally switched to GNU, switch back:

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

### Full steps (one-shot setup on a new machine)

```powershell
# 1. Install MSVC C++ Build Tools (includes link.exe)
winget install Microsoft.VisualStudio.2022.BuildTools --source winget --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"

# 2. Make sure the default toolchain is MSVC
rustup default stable-x86_64-pc-windows-msvc

# 3. Open a new terminal and verify
cargo test -p desk-core
```

## Verification

```powershell
cd .\DSH-desk
cargo test -p desk-core
```

When `test result: ok. 37 passed` appears, the installation is successful.
