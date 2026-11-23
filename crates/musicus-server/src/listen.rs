//! The listening socket (a-dual-stack-listen).
//!
//! The default address `[::]:8080` is bound as one dual-stack socket that
//! accepts IPv6 and IPv4 (IPv4 clients appear as `::ffff:a.b.c.d`), so a
//! reverse proxy may connect to `localhost` whichever of `::1` and
//! `127.0.0.1` it picks. `IPV6_V6ONLY` is switched off explicitly because
//! Windows and some BSDs switch it on by default. Where IPv6 is missing
//! (kernel `ipv6.disable=1`, some containers), the unspecified `[::]` falls
//! back to `0.0.0.0` with the same port; every other address is bound
//! exactly as given.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};

/// How the listener was bound, for the start-up log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    /// `[::]` accepting IPv6 and IPv4.
    Dual,
    /// Exactly the given address (one family only).
    AsGiven,
    /// `[::]` failed; bound `0.0.0.0` instead (IPv4 only).
    Ipv4Fallback,
}

impl Stack {
    pub fn describe(self) -> &'static str {
        match self {
            Stack::Dual => "IPv6 and IPv4",
            Stack::AsGiven => "as configured",
            Stack::Ipv4Fallback => "IPv4 only, IPv6 unavailable",
        }
    }
}

/// Binds `addr` (non-blocking, ready for tokio / axum_server).
pub fn bind(addr: SocketAddr) -> io::Result<(TcpListener, Stack)> {
    bind_with(addr, bind_socket)
}

/// [`bind`] with an injectable socket function (tests simulate a host without IPv6).
pub fn bind_with(
    addr: SocketAddr,
    bind_socket: impl Fn(SocketAddr) -> io::Result<TcpListener>,
) -> io::Result<(TcpListener, Stack)> {
    if addr.ip() != Ipv6Addr::UNSPECIFIED {
        return Ok((bind_socket(addr)?, Stack::AsGiven));
    }
    match bind_socket(addr) {
        Ok(listener) => Ok((listener, Stack::Dual)),
        // Would hit IPv4 the same way; report the real cause.
        Err(e) if matches!(e.kind(), io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied) => Err(e),
        Err(e) => {
            let v4 = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), addr.port());
            tracing::warn!("cannot listen on {addr} ({e}); IPv6 unavailable, listening on {v4} (IPv4 only)");
            Ok((bind_socket(v4)?, Stack::Ipv4Fallback))
        }
    }
}

/// One socket like `tokio::net::TcpListener::bind` (SO_REUSEADDR except on
/// Windows, backlog 1024), plus `IPV6_V6ONLY = false` for unspecified `[::]`.
fn bind_socket(addr: SocketAddr) -> io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    if addr.ip() == Ipv6Addr::UNSPECIFIED {
        socket.set_only_v6(false)?;
    }
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

/// Where a client on this machine reaches a server listening on `listen`:
/// unspecified addresses map to loopback (`[::]` → `[::1]`, then `127.0.0.1`,
/// because the server may have fallen back to IPv4; `0.0.0.0` → `127.0.0.1`).
pub fn local_targets(listen: SocketAddr) -> Vec<SocketAddr> {
    let port = listen.port();
    let v4 = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    match listen.ip() {
        ip if ip == Ipv6Addr::UNSPECIFIED => vec![SocketAddr::new(Ipv6Addr::LOCALHOST.into(), port), v4],
        ip if ip == Ipv4Addr::UNSPECIFIED => vec![v4],
        _ => vec![listen],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    fn any(port: u16) -> SocketAddr {
        SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port)
    }

    /// Accepts one connection and answers it, so a connect really reached this listener.
    fn reaches(listener: &TcpListener, target: SocketAddr) -> bool {
        let Ok(mut client) = TcpStream::connect_timeout(&target, Duration::from_secs(2)) else { return false };
        listener.set_nonblocking(false).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.write_all(b"x").unwrap();
        let mut buf = [0u8; 1];
        client.read_exact(&mut buf).unwrap();
        listener.set_nonblocking(true).unwrap();
        buf == *b"x"
    }

    fn host_has_ipv6() -> bool {
        TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok()
    }

    #[test]
    fn default_accepts_ipv4_and_ipv6() {
        let (listener, stack) = bind(any(0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        if stack == Stack::Ipv4Fallback || !host_has_ipv6() {
            eprintln!("no IPv6 on this host: checking IPv4 only");
        } else {
            assert_eq!(stack, Stack::Dual);
            assert!(reaches(&listener, (Ipv6Addr::LOCALHOST, port).into()), "::1");
        }
        assert!(reaches(&listener, (Ipv4Addr::LOCALHOST, port).into()), "127.0.0.1");
    }

    #[test]
    fn falls_back_to_ipv4_when_ipv6_is_missing() {
        let no_v6 = |a: SocketAddr| {
            if a.is_ipv6() { Err(io::Error::from(io::ErrorKind::Unsupported)) } else { bind_socket(a) }
        };
        let (listener, stack) = bind_with(any(0), no_v6).unwrap();
        assert_eq!(stack, Stack::Ipv4Fallback);
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip(), Ipv4Addr::UNSPECIFIED);
        assert!(reaches(&listener, (Ipv4Addr::LOCALHOST, addr.port()).into()));
    }

    #[test]
    fn keeps_the_port_on_fallback_and_never_falls_back_when_busy() {
        let tried = std::cell::RefCell::new(Vec::new());
        let fail = |kind: io::ErrorKind| {
            let tried = &tried;
            move |a: SocketAddr| -> io::Result<TcpListener> {
                tried.borrow_mut().push(a);
                Err(io::Error::from(kind))
            }
        };
        assert!(bind_with(any(8123), fail(io::ErrorKind::AddrNotAvailable)).is_err());
        assert_eq!(*tried.borrow(), [any(8123), SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8123))]);
        tried.borrow_mut().clear();
        let err = bind_with(any(8123), fail(io::ErrorKind::AddrInUse)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(*tried.borrow(), [any(8123)]);
    }

    #[test]
    fn explicit_addresses_are_bound_as_given() {
        let tried = std::cell::RefCell::new(Vec::new());
        let refuse = |a: SocketAddr| -> io::Result<TcpListener> {
            tried.borrow_mut().push(a);
            Err(io::Error::from(io::ErrorKind::AddrNotAvailable))
        };
        for given in ["127.0.0.1:8080", "[::1]:8080", "0.0.0.0:8080", "192.0.2.7:8080"] {
            let addr: SocketAddr = given.parse().unwrap();
            assert!(bind_with(addr, refuse).is_err(), "{given}: no fallback");
        }
        assert_eq!(tried.borrow().len(), 4);

        let (listener, stack) = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        assert_eq!(stack, Stack::AsGiven);
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
        if host_has_ipv6() {
            assert!(!reaches(&listener, (Ipv6Addr::LOCALHOST, addr.port()).into()), "127.0.0.1 is IPv4 only");
            let (v6, _) = bind("[::1]:0".parse().unwrap()).unwrap();
            let port = v6.local_addr().unwrap().port();
            assert!(!reaches(&v6, (Ipv4Addr::LOCALHOST, port).into()), "[::1] is IPv6 only");
            assert!(reaches(&v6, (Ipv6Addr::LOCALHOST, port).into()));
        }
    }

    #[test]
    fn local_targets_map_unspecified_to_loopback() {
        let t = |s: &str| local_targets(s.parse().unwrap());
        assert_eq!(t("[::]:8080"), ["[::1]:8080".parse().unwrap(), "127.0.0.1:8080".parse().unwrap()]);
        assert_eq!(t("0.0.0.0:9000"), ["127.0.0.1:9000".parse().unwrap()]);
        assert_eq!(t("127.0.0.1:8080"), ["127.0.0.1:8080".parse().unwrap()]);
        assert_eq!(t("[::1]:8080"), ["[::1]:8080".parse().unwrap()]);
        assert_eq!(t("192.0.2.7:8080"), ["192.0.2.7:8080".parse().unwrap()]);
    }
}
