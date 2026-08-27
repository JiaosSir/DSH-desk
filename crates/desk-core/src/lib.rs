//! desk-core：sidecar 监督、端口协商、profile 初始化与滚动日志。
//! 纯 Rust、无 Tauri 依赖，所有行为可在无窗口环境单测。

pub mod config;
pub mod credentials;
pub mod logs;
pub mod paths;
pub mod ports;
pub mod profile;
pub mod ready;
pub mod sidecar_cache;
pub mod supervisor;
