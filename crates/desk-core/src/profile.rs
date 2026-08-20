//! Desktop profile initialization and the web→desktop plugin sync diff.
//!
//! Phase 3 fills this in: [`ensure_profile_init`](crate::profile::ensure_profile_init)
//! (idempotent `dsh plugin --profile desktop add` runs through the bundled
//! node+pnpm) and [`compute_sync_diff`](crate::profile::compute_sync_diff).
