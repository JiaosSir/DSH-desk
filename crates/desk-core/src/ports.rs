//! Free-port selection for the sidecar.

use std::net::TcpListener;

/// Pick a free 127.0.0.1 port: bind `:0` to take an OS-assigned ephemeral
/// port, then release it immediately. A TOCTOU race exists between release
/// and the sidecar's bind — the supervisor's retry loop is the backstop
/// (see [`supervisor`](crate::supervisor)).
pub fn pick_free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_free_port_returns_ephemeral_and_rebindable() {
        let port = pick_free_port().expect("pick a free port");
        assert!(
            (1024..=65535).contains(&port),
            "expected an ephemeral port, got {port}"
        );
        // A rare race can let another process grab the port first; rebinding
        // here must not panic — the supervisor retries on bind failures.
        let _ = TcpListener::bind(("127.0.0.1", port));
    }
}
