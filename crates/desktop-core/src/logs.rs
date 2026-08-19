//! Rolling log writer for the sidecar and shell logs.
//!
//! Phase 2 fills this in: a timestamped line writer that rotates the active
//! file once it exceeds 1 MiB (keeping one `.1` archive).
