//! 滚动日志写入器（1 MiB × 2 份）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单文件大小上限（1 MiB）。
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// 时间戳行写入器：基础文件满 1 MiB 时轮转为归档（覆盖旧档），继续写基础文件；
/// 日期由调用方在启动时决定（新的一天、新文件）。
pub struct RollingLog {
    base: PathBuf,
    archived: PathBuf,
}

impl RollingLog {
    /// 在 `dir` 下创建滚动日志（目录不存在时自动创建）；`name` 形如 `sidecar`，`date` 形如 `20260819`。
    pub fn new(dir: &Path, name: &str, date: &str) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            base: dir.join(format!("{name}-{date}.log")),
            archived: dir.join(format!("{name}-{date}.1.log")),
        })
    }

    /// 基础日志文件路径（错误页「打开日志目录」用）。
    pub fn base_path(&self) -> &Path {
        &self.base
    }

    /// 追加一行（自动加毫秒时间戳前缀）。写入前若基础文件已满则先轮转。
    pub fn append(&self, line: &str) -> std::io::Result<()> {
        if self.base.exists() && self.base.metadata()?.len() >= MAX_FILE_BYTES {
            let _ = fs::remove_file(&self.archived);
            let _ = fs::rename(&self.base, &self.archived);
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.base)?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        writeln!(file, "{millis} {line}")?;
        Ok(())
    }
}

/// 当前本地日期的紧凑形式（`YYYYMMDD`，日志文件名用）；用 civil-from-days 算法换算，免引日期依赖。
pub fn today_compact() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    // 1970-01-01 起的天数 → 公历年月日。
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 用一条超大行撑爆 1 MiB 阈值，验证下一次写入前触发轮转。
    #[test]
    fn 超限触发轮转且旧档保留早期内容() {
        let dir = tempfile::tempdir().expect("临时目录");
        let log = RollingLog::new(dir.path(), "sidecar", "20260819").expect("创建日志");

        // 两条小行 + 一条超过阈值的行：写大行之前文件很小，不会轮转。
        log.append("first").expect("写 first");
        log.append("second").expect("写 second");
        let big = "x".repeat(MAX_FILE_BYTES as usize + 10);
        log.append(&big).expect("写超大行");

        // 此时基础文件已超限；下一次写入前轮转：归档保留早期内容，
        // 基础文件只剩最新一行。
        log.append("after").expect("写 after");

        let archived = fs::read_to_string(&log.archived).expect("读归档");
        assert!(archived.contains("first"), "归档应保留早期内容");
        assert!(archived.contains("second"), "归档应保留早期内容");
        assert!(archived.contains(&big), "归档应保留超限行");

        let base = fs::read_to_string(&log.base).expect("读基础文件");
        assert!(base.trim_end().ends_with("after"), "基础文件应含最新行");
        assert!(!base.contains("first"), "基础文件不应再含旧行");
        let base_len = fs::metadata(&log.base).expect("stat").len();
        assert!(base_len < MAX_FILE_BYTES, "基础文件应低于阈值");
    }

    #[test]
    fn 追加行带时间戳前缀() {
        let dir = tempfile::tempdir().expect("临时目录");
        let log = RollingLog::new(dir.path(), "app", "20260819").expect("创建日志");
        log.append("hello").expect("写一行");
        let content = fs::read_to_string(&log.base).expect("读基础文件");
        assert!(content.trim_end().ends_with("hello"), "行尾应为原文");
        assert!(
            content
                .split_whitespace()
                .next()
                .unwrap_or("")
                .parse::<u128>()
                .is_ok(),
            "行首应为毫秒时间戳"
        );
    }

    #[test]
    fn 今日日期为八位数字() {
        let today = today_compact();
        assert_eq!(today.len(), 8, "日期应为 YYYYMMDD: {today}");
        assert!(today.chars().all(|c| c.is_ascii_digit()));
        // 2026 年的合理范围（测试运行于 2026-08）。
        assert!(today.starts_with("2026"), "应为当前年份: {today}");
    }
}
