//! The URL a client on the LAN opens for a server bound to a given address.
//!
//! A server bound to the loopback is reached from this machine only, so it
//! has no LAN URL. A server bound to one address is reached at that address.
//! A server bound to every interface is reached at the address this machine
//! sends from toward the network, which is what `ip route get` reports.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

/// An address in the range reserved for documentation (RFC 5737); no host
/// owns it. Only the routing table is asked about it.
const PROBE_V4: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
/// The same for IPv6 (RFC 3849).
const PROBE_V6: Ipv6Addr = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1);
/// The discard port. A connected UDP socket sends nothing until it is written to.
const PROBE_PORT: u16 = 9;

/// Where a client on the LAN reaches a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanUrl {
    /// The server is bound to the loopback; no client on the LAN can connect.
    Loopback,
    /// The URL a client on the LAN opens.
    At(String),
    /// The server is bound to every interface, but this machine has no route
    /// toward a network, so no address can be named.
    Unknown,
}

/// The address this machine sends from toward a network, in the family of
/// `any`. `None` when there is no route, or when the route leaves from the
/// loopback.
fn outbound_address(any: IpAddr) -> Option<IpAddr> {
    let probe = match any {
        IpAddr::V4(_) => IpAddr::V4(PROBE_V4),
        IpAddr::V6(_) => IpAddr::V6(PROBE_V6),
    };
    let socket = UdpSocket::bind((any, 0)).ok()?;
    socket.connect((probe, PROBE_PORT)).ok()?;
    let found = socket.local_addr().ok()?.ip();
    (!found.is_loopback() && !found.is_unspecified()).then_some(found)
}

/// The URL for `path` on a server that listens on `bound`.
pub fn lan_url(bound: SocketAddr, path: &str) -> LanUrl {
    let ip = bound.ip();
    if ip.is_loopback() {
        return LanUrl::Loopback;
    }
    let host = if ip.is_unspecified() {
        outbound_address(ip)
    } else {
        Some(ip)
    };
    match host {
        Some(host) => LanUrl::At(format!(
            "http://{}{path}",
            SocketAddr::new(host, bound.port())
        )),
        None => LanUrl::Unknown,
    }
}

/// The `scope` a log event names for a server bound to `ip`.
pub fn scope_of(ip: IpAddr) -> &'static str {
    if ip.is_loopback() { "loopback" } else { "lan" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket_at(text: &str, port: u16) -> Result<SocketAddr, std::net::AddrParseError> {
        Ok(SocketAddr::new(text.parse::<IpAddr>()?, port))
    }

    #[test]
    fn a_server_on_the_loopback_has_no_lan_url() -> Result<(), std::net::AddrParseError> {
        assert_eq!(
            lan_url(socket_at("127.0.0.1", 8833)?, "/"),
            LanUrl::Loopback
        );
        assert_eq!(lan_url(socket_at("::1", 8833)?, "/"), LanUrl::Loopback);
        Ok(())
    }

    #[test]
    fn a_server_on_one_address_is_reached_at_that_address() -> Result<(), std::net::AddrParseError>
    {
        assert_eq!(
            lan_url(socket_at("192.168.200.20", 8833)?, "/"),
            LanUrl::At(String::from("http://192.168.200.20:8833/"))
        );
        assert_eq!(
            lan_url(socket_at("192.168.200.20", 8835)?, "/events"),
            LanUrl::At(String::from("http://192.168.200.20:8835/events"))
        );
        Ok(())
    }

    #[test]
    fn an_ipv6_address_is_written_in_brackets() -> Result<(), std::net::AddrParseError> {
        assert_eq!(
            lan_url(socket_at("fd00::5", 8833)?, "/"),
            LanUrl::At(String::from("http://[fd00::5]:8833/"))
        );
        Ok(())
    }

    #[test]
    fn a_server_on_every_interface_never_names_the_wildcard_or_the_loopback()
    -> Result<(), std::net::AddrParseError> {
        // Which of the two answers a machine gives depends on its routing table.
        let found = lan_url(socket_at("0.0.0.0", 8833)?, "/");
        assert_ne!(found, LanUrl::Loopback);
        if let LanUrl::At(url) = found {
            assert!(
                url.starts_with("http://") && url.ends_with(":8833/"),
                "{url}"
            );
            assert!(!url.contains("0.0.0.0") && !url.contains("127."), "{url}");
        }
        Ok(())
    }

    #[test]
    fn the_scope_names_the_loopback_and_everything_else() -> Result<(), std::net::AddrParseError> {
        assert_eq!(scope_of("127.0.0.1".parse()?), "loopback");
        assert_eq!(scope_of("::1".parse()?), "loopback");
        assert_eq!(scope_of("0.0.0.0".parse()?), "lan");
        assert_eq!(scope_of("192.168.200.20".parse()?), "lan");
        Ok(())
    }
}
