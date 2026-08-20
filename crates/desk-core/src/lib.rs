//! desk-core: sidecar supervision, port negotiation, profile
//! initialization, and rolling logs. Pure Rust with no Tauri dependency so
//! every behavior is unit-testable without a windowing environment.

pub mod logs;
pub mod paths;
pub mod ports;
pub mod profile;
pub mod ready;
pub mod supervisor;
