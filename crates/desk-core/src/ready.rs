//! sidecar 就绪行解析。

use std::sync::OnceLock;

/// 匹配就绪 URL 行的正则（与官方 e2e 同构）。
fn ready_url_regex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"dsh web: (http://[^\s]+)").expect("常量正则必须合法"))
}

/// 从累计输出中抓取就绪 URL（`dsh web: <url>` 行；捕获组只取第一个 URL，LAN 后缀不影响）。
pub fn extract_ready_url(accumulated: &str) -> Option<String> {
    ready_url_regex()
        .captures(accumulated)?
        .get(1)
        .map(|m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 提取普通就绪行() {
        let out = "boot noise…\ndsh web: http://127.0.0.1:43210\n";
        assert_eq!(
            extract_ready_url(out).as_deref(),
            Some("http://127.0.0.1:43210")
        );
    }

    #[test]
    fn 提取带_lan_后缀的就绪行() {
        let out = "dsh web: http://127.0.0.1:3080 (LAN: http://192.168.1.5:3080)\n";
        assert_eq!(
            extract_ready_url(out).as_deref(),
            Some("http://127.0.0.1:3080")
        );
    }

    #[test]
    fn 就绪前返回_none() {
        assert_eq!(extract_ready_url("starting…"), None);
    }
}
