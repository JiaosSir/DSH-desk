//! desk-core: sidecar supervision, port negotiation, profile
//! initialization, and rolling logs. Pure Rust with no Tauri dependency so
//! every behavior is unit-testable without a windowing environment.

pub mod config;
pub mod credentials;
pub mod logs;
pub mod paths;
pub mod ports;
pub mod profile;
pub mod ready;
pub mod sidecar_cache;
pub mod supervisor;
