use std::net::{Ipv4Addr, TcpListener};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PortCheck {
    pub port: u16,
    pub free: bool,
    pub suggestion: Option<u16>,
}

/// Services bind 127.0.0.1, so that is what we probe. A socket bound to
/// 0.0.0.0 on the same port also makes this bind fail.
pub fn is_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Next free port after `port`, skipping any in `reserved`.
pub fn suggest(port: u16, reserved: &[u16]) -> Option<u16> {
    let start = port.max(1023).saturating_add(1);
    (start..=start.saturating_add(200).min(u16::MAX))
        .find(|p| !reserved.contains(p) && is_free(*p))
}

pub fn check(port: u16, reserved: &[u16]) -> PortCheck {
    let free = is_free(port);
    PortCheck {
        port,
        free,
        suggestion: if free { None } else { suggest(port, reserved) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_busy_port_and_suggests() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let c = check(port, &[]);
        assert!(!c.free);
        let s = c.suggestion.unwrap();
        assert_ne!(s, port);
        assert!(is_free(s));
        drop(l);
        assert!(is_free(port));
    }
}
