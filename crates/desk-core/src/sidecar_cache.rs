//! sidecar 打包资产缓存（方案 A）：release 构建只携带 `sidecar-dist.tar`
//! （未压缩 tar）与 `sidecar-version.json`；首次运行把 tar 解压到用户级
//! 缓存目录，按 VERSION.json 幂等（版本一致直接复用，否则重解）。
//! 纯函数、无 Tauri 依赖，可在无窗口环境单测。

use std::cell::Cell;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

/// 期望版本（来自 sidecar-version.json，与 VERSION.json 同构）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarVersion {
    pub node: String,
    pub pnpm: String,
    pub dsh: String,
}

/// 读版本文件（`sidecar-version.json` 与 `sidecar-dist/VERSION.json` 同构；
/// 额外字段如 `assembledAt` 忽略）。
pub fn read_version_file(path: &Path) -> Result<SidecarVersion, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", path.display()))?;
    let field = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("{} 缺少字符串字段 {name}", path.display()))
    };
    Ok(SidecarVersion {
        node: field("node")?,
        pnpm: field("pnpm")?,
        dsh: field("dsh")?,
    })
}

/// 缓存目录里的 VERSION.json 是否与期望版本一致。
pub fn cache_matches(cache_dir: &Path, expected: &SidecarVersion) -> bool {
    read_version_file(&cache_dir.join("VERSION.json"))
        .map(|v| &v == expected)
        .unwrap_or(false)
}

/// env `DSH_DESK_SIDECAR_CACHE` 覆盖缓存目录（冒烟/调试用，与 `SIDECAR_ROOT` 同风格）。
pub fn sidecar_cache_override() -> Option<PathBuf> {
    std::env::var_os("DSH_DESK_SIDECAR_CACHE").map(PathBuf::from)
}

/// 确保 sidecar 缓存就绪：版本一致直接返回；否则清理残留 tmp、解压 tar 到
/// `<cache>.tmp-<pid>`、校验产物、删旧缓存、rename 提交。进度经 `on_progress`
/// 上报（0.0..=1.0，已解压字节 / tar 字节，无需预扫条目数）。
pub fn ensure_cached_sidecar(
    archive: &Path,
    version_file: &Path,
    cache_dir: &Path,
    mut on_progress: impl FnMut(f64) + Send,
) -> Result<PathBuf, String> {
    let expected = read_version_file(version_file)?;
    if cache_matches(cache_dir, &expected) {
        on_progress(1.0);
        return Ok(cache_dir.to_owned());
    }

    let archive_len = fs::metadata(archive)
        .map_err(|e| format!("读取打包资产失败 {}: {e}", archive.display()))?
        .len();
    if archive_len == 0 {
        return Err(format!("打包资产为空: {}", archive.display()));
    }

    cleanup_stale_tmp(cache_dir);

    let tmp_dir = cache_dir.with_file_name(format!(
        "{}.tmp-{}",
        cache_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sidecar-dist"),
        std::process::id(),
    ));

    if let Err(e) = extract_tar(archive, &tmp_dir, archive_len, &mut on_progress) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(e);
    }

    // 产物校验：解出的 VERSION.json 必须与期望版本一致（防半截/错包）。
    if !cache_matches(&tmp_dir, &expected) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err("解压产物 VERSION.json 与期望版本不一致".to_owned());
    }

    // 提交：删旧缓存 → rename（Windows 上 rename 要求目标不存在，故先删）。
    if cache_dir.exists() {
        fs::remove_dir_all(cache_dir)
            .map_err(|e| format!("清理旧缓存失败 {}: {e}", cache_dir.display()))?;
    }
    if let Some(parent) = cache_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建缓存父目录 {} 失败: {e}", parent.display()))?;
    }
    fs::rename(&tmp_dir, cache_dir).map_err(|e| {
        format!(
            "缓存提交失败（{} → {}）: {e}",
            tmp_dir.display(),
            cache_dir.display()
        )
    })?;
    on_progress(1.0);
    Ok(cache_dir.to_owned())
}

/// 清理历史中断遗留的 `<cache>.tmp-*` 目录（幂等）。
fn cleanup_stale_tmp(cache_dir: &Path) {
    let parent = cache_dir.parent().unwrap_or_else(|| Path::new("."));
    let base = cache_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sidecar-dist");
    let prefix = format!("{base}.tmp-");
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

/// 条目路径安全判定：拒绝绝对路径、盘符/UNC 前缀、根目录段与任何 `..` 段
/// （防 tar 穿越；自产 tar 属防御性）。Windows 上 `/abs`、`\abs` 会被
/// `is_absolute` 判为非绝对，但带 RootDir 段，必须一并拒绝。
fn safe_entry_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

/// 带进度计数的读取器：统计 tar 流实际消费的字节数。
struct ProgressReader {
    inner: File,
    read: Rc<Cell<u64>>,
}

impl Read for ProgressReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.set(self.read.get() + n as u64);
        Ok(n)
    }
}

/// 把 tar 解压到 `dest`；每条条目落盘后按字节比例上报进度。
/// 注意：pnpm 的 node_modules 大量使用硬链接，bsdtar 会把同内容文件存成
/// 硬链接条目（size 0 + link_name）——必须还原为硬链接，否则目标文件是
/// 空文件（宿主解析 package.json 即崩）。符号链接为防御性支持。
fn extract_tar(
    archive: &Path,
    dest: &Path,
    archive_len: u64,
    on_progress: &mut (impl FnMut(f64) + Send),
) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("创建缓存目录失败 {}: {e}", dest.display()))?;
    let file =
        File::open(archive).map_err(|e| format!("打开打包资产失败 {}: {e}", archive.display()))?;
    let read = Rc::new(Cell::new(0u64));
    let mut reader = ProgressReader {
        inner: file,
        read: Rc::clone(&read),
    };
    let mut builder = tar::Archive::new(&mut reader);
    for entry in builder.entries().map_err(|e| format!("读取 tar 失败: {e}"))? {
        let mut entry = entry.map_err(|e| format!("读取 tar 条目失败: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("tar 条目路径非法: {e}"))?;
        if !safe_entry_path(&path) {
            return Err(format!("tar 条目路径非法（穿越风险）: {}", path.display()));
        }
        let out = dest.join(&path);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&out)
                .map_err(|e| format!("创建目录失败 {}: {e}", out.display()))?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .map_err(|e| format!("读取链接目标失败 {}: {e}", path.display()))?
                .ok_or_else(|| format!("链接条目缺少目标: {}", path.display()))?;
            let target_path = target.as_ref();
            if !safe_entry_path(target_path) {
                return Err(format!(
                    "链接目标非法（穿越风险）: {} → {}",
                    path.display(),
                    target_path.display()
                ));
            }
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败 {}: {e}", parent.display()))?;
            }
            let _ = fs::remove_file(&out);
            if kind.is_hard_link() {
                fs::hard_link(dest.join(target_path), &out).map_err(|e| {
                    format!(
                        "创建硬链接失败 {} → {}: {e}",
                        out.display(),
                        target_path.display()
                    )
                })?;
            } else {
                create_symlink(target_path, dest, &out)?;
            }
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录失败 {}: {e}", parent.display()))?;
            }
            let mut f =
                File::create(&out).map_err(|e| format!("创建文件失败 {}: {e}", out.display()))?;
            io::copy(&mut entry, &mut f)
                .map_err(|e| format!("解压失败 {}: {e}", out.display()))?;
        }
        on_progress((read.get() as f64 / archive_len as f64).min(1.0));
    }
    Ok(())
}

/// 防御性符号链接创建（自产 tar 应无符号链接；Windows 无特权时可能失败）。
#[cfg(windows)]
fn create_symlink(target: &Path, dest: &Path, out: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(dest.join(target), out).map_err(|e| {
        format!(
            "创建符号链接失败 {} → {}: {e}（Windows 需要开发者模式或管理员权限）",
            out.display(),
            target.display()
        )
    })
}

#[cfg(not(windows))]
fn create_symlink(target: &Path, _dest: &Path, out: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, out).map_err(|e| {
        format!(
            "创建符号链接失败 {} → {}: {e}",
            out.display(),
            target.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存里构造一个小 tar 夹具。
    fn tar_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, *name, *data).expect("fixture 追加");
        }
        builder.into_inner().expect("fixture 收尾")
    }

    fn write_asset(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).expect("写夹具");
        path
    }

    const VERSION_JSON: &str = r#"{"node":"24.19.0","pnpm":"11.7.0","dsh":"0.1.1-rc.2","assembledAt":"x"}"#;

    fn progress() -> Vec<f64> {
        Vec::new()
    }

    #[test]
    fn 读取版本文件与缓存判定() {
        let tmp = tempfile::TempDir::new().unwrap();
        let version_file = write_asset(tmp.path(), "sidecar-version.json", VERSION_JSON.as_bytes());
        let expected = read_version_file(&version_file).unwrap();
        assert_eq!(expected.dsh, "0.1.1-rc.2");

        let cache = tmp.path().join("cache");
        assert!(!cache_matches(&cache, &expected), "缓存缺失不匹配");
        fs::create_dir_all(&cache).unwrap();
        write_asset(&cache, "VERSION.json", VERSION_JSON.as_bytes());
        assert!(cache_matches(&cache, &expected), "版本一致应匹配");
        write_asset(
            &cache,
            "VERSION.json",
            br#"{"node":"24.19.0","pnpm":"9.9.9","dsh":"0.1.1-rc.2"}"#,
        );
        assert!(!cache_matches(&cache, &expected), "版本漂移不匹配");
        write_asset(&cache, "VERSION.json", b"{ broken");
        assert!(!cache_matches(&cache, &expected), "损坏 VERSION.json 不匹配");
    }

    #[test]
    fn 首次解压后命中缓存_再删归档也成功() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = [
            ("node/node.exe", &b"exe"[..]),
            ("VERSION.json", VERSION_JSON.as_bytes()),
        ];
        let archive = write_asset(tmp.path(), "sidecar-dist.tar", &tar_fixture(&entries));
        let version_file = write_asset(tmp.path(), "sidecar-version.json", VERSION_JSON.as_bytes());
        let cache = tmp.path().join("cache");

        let mut calls = progress();
        let out = ensure_cached_sidecar(&archive, &version_file, &cache, |p| calls.push(p)).unwrap();
        assert_eq!(out, cache);
        assert!(cache.join("node/node.exe").exists(), "node.exe 应解出");
        assert!(calls.last().copied().unwrap_or(0.0) > 0.9, "进度应收尾到 1.0");
        assert!(
            calls.windows(2).all(|w| w[0] <= w[1]),
            "进度应单调非降"
        );

        // 删除归档后再次调用：命中缓存，不再需要归档。
        fs::remove_file(&archive).unwrap();
        let out2 = ensure_cached_sidecar(&archive, &version_file, &cache, |_| {}).unwrap();
        assert_eq!(out2, cache, "二次调用应命中缓存");
    }

    #[test]
    fn 版本不符时重新解压() {
        let tmp = tempfile::TempDir::new().unwrap();
        let v1 = br#"{"node":"24.19.0","pnpm":"11.7.0","dsh":"0.1.1-rc.1"}"#;
        let v2 = VERSION_JSON;
        let archive = write_asset(
            tmp.path(),
            "sidecar-dist.tar",
            &tar_fixture(&[("VERSION.json", &v1[..]), ("a.txt", b"v1")]),
        );
        let version_file = write_asset(tmp.path(), "sidecar-version.json", &v1[..]);
        let cache = tmp.path().join("cache");
        ensure_cached_sidecar(&archive, &version_file, &cache, |_| {}).unwrap();
        assert_eq!(fs::read_to_string(cache.join("a.txt")).unwrap(), "v1");

        // 归档与版本文件都换成 v2 → 应重解。
        let archive2 = write_asset(
            tmp.path(),
            "sidecar-dist2.tar",
            &tar_fixture(&[("VERSION.json", v2.as_bytes()), ("a.txt", b"v2")]),
        );
        write_asset(tmp.path(), "sidecar-version.json", v2.as_bytes());
        ensure_cached_sidecar(&archive2, &version_file, &cache, |_| {}).unwrap();
        assert_eq!(fs::read_to_string(cache.join("a.txt")).unwrap(), "v2");
    }

    #[test]
    fn 损坏归档返回错误并清理_tmp() {
        let tmp = tempfile::TempDir::new().unwrap();
        let archive = write_asset(tmp.path(), "sidecar-dist.tar", b"not-a-tar");
        let version_file = write_asset(tmp.path(), "sidecar-version.json", VERSION_JSON.as_bytes());
        let cache = tmp.path().join("cache");

        let err = ensure_cached_sidecar(&archive, &version_file, &cache, |_| {}).unwrap_err();
        assert!(err.contains("tar"), "错误应来自 tar 解析: {err}");
        assert!(!cache.exists(), "失败后不应有正式缓存");
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("cache.tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "失败后 tmp 目录应被清理");
    }

    #[test]
    fn 硬链接条目还原为硬链接而非空文件() {
        let tmp = tempfile::TempDir::new().unwrap();
        // 夹具：a.txt 数据条目 + b.txt 硬链接条目（size 0 + link_name）+
        // VERSION.json。bsdtar 对 pnpm 硬链接文件就是这么存的。
        let mut builder = tar::Builder::new(Vec::new());
        let mut data = tar::Header::new_gnu();
        data.set_size(7);
        data.set_mode(0o644);
        data.set_cksum();
        builder
            .append_data(&mut data, "a.txt", &b"content"[..])
            .expect("数据条目");
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Link);
        link.set_size(0);
        link.set_mode(0o644);
        link.set_cksum();
        builder
            .append_link(&mut link, "b.txt", "a.txt")
            .expect("硬链接条目");
        let mut vh = tar::Header::new_gnu();
        vh.set_size(VERSION_JSON.len() as u64);
        vh.set_mode(0o644);
        vh.set_cksum();
        builder
            .append_data(&mut vh, "VERSION.json", VERSION_JSON.as_bytes())
            .expect("版本条目");
        let fixture = builder.into_inner().expect("夹具收尾");

        let archive = write_asset(tmp.path(), "sidecar-dist.tar", &fixture);
        let version_file =
            write_asset(tmp.path(), "sidecar-version.json", VERSION_JSON.as_bytes());
        let cache = tmp.path().join("cache");
        ensure_cached_sidecar(&archive, &version_file, &cache, |_| {}).unwrap();

        assert_eq!(
            fs::read_to_string(cache.join("a.txt")).unwrap(),
            "content",
            "数据条目应完整"
        );
        assert_eq!(
            fs::read_to_string(cache.join("b.txt")).unwrap(),
            "content",
            "硬链接条目必须有内容（不能是空文件）"
        );
    }

    #[test]
    fn 路径穿越条目被拒绝() {
        assert!(safe_entry_path(Path::new("a/b/c")));
        assert!(safe_entry_path(Path::new("VERSION.json")));
        assert!(!safe_entry_path(Path::new("../evil.txt")));
        assert!(!safe_entry_path(Path::new("a/../b")));
        assert!(!safe_entry_path(Path::new("/abs")));
        assert!(!safe_entry_path(Path::new("..\\evil.txt")), "Windows 反斜杠同样拒绝");
    }
}
