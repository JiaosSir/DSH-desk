//! DeepSeek API key 存在性检测（首次引导用）：凭证走 harness 现有机制
//! （`<home>/.env` 或 `<home>/.credentials.yaml`），只判键名是否存在、不读值，
//! 值永远不进入桌面进程。

use std::path::Path;

/// 凭证文件候选（与 harness 现有机制一致）。
const CREDENTIAL_FILES: [&str; 2] = [".env", ".credentials.yaml"];

/// dsh 主目录里是否已配置 DeepSeek API key（任意候选文件文本含键名即视为已配置）。
pub fn has_api_key(dsh_home: &Path) -> bool {
    CREDENTIAL_FILES.iter().any(|name| {
        std::fs::read_to_string(dsh_home.join(name))
            .map(|text| text.contains("DEEPSEEK_API_KEY"))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn 无任何凭证文件为_false() {
        let tmp = TempDir::new().unwrap();
        assert!(!has_api_key(tmp.path()));
    }

    #[test]
    fn dot_env_含键名为_true() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".env"), "DEEPSEEK_API_KEY=sk-xxxx\n").unwrap();
        assert!(has_api_key(tmp.path()));
    }

    #[test]
    fn credentials_yaml_含键名为_true() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join(".credentials.yaml"),
            "deepseek:\n  DEEPSEEK_API_KEY: sk-xxxx\n",
        )
        .unwrap();
        assert!(has_api_key(tmp.path()));
    }

    #[test]
    fn 文件存在但不含键名为_false() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".env"), "OTHER_KEY=1\n").unwrap();
        assert!(!has_api_key(tmp.path()));
    }
}
