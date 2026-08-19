//! Ready-line parsing from the sidecar's stdout.
//!
//! Phase 2 fills this in: [`extract_ready_url`](crate::ready::extract_ready_url)
//! matching the harness URL line `dsh web: http://127.0.0.1:<port>` (with an
//! optional ` (LAN: …)` suffix).
