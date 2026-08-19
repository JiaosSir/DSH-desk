//! Sidecar supervision state machine.
//!
//! Phase 2 fills this in: spawn → wait for the ready URL line (90s timeout) →
//! running; crash backoff restarts up to 3 attempts; failure surfaces a
//! terminal state the shell renders as its error page.
