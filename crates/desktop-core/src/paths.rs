//! DSH_HOME resolution and desktop-owned path layout.
//!
//! Phase 2 fills this in: [`dsh_home`](crate::paths::dsh_home) (DSH_HOME env
//! first, then `~/.dsh`), the desktop profile dir, the desktop config dir,
//! and the logs dir.
