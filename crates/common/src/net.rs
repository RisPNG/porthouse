//! Socket settings shared by the server and the client.

use std::io;
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};

/// How long a connection may stay silent before probing, and how often to probe after that.
pub const KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);

fn keepalive() -> TcpKeepalive {
    let ka = TcpKeepalive::new().with_time(KEEPALIVE_IDLE).with_interval(KEEPALIVE_INTERVAL);
    // Linux and the BSDs let us bound the number of probes; Windows uses a fixed count.
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd"))]
    let ka = ka.with_retries(3);
    ka
}

/// Turn on TCP keepalive so a peer that vanished without closing the connection (a machine that
/// slept, crashed or lost its network) is noticed within about 25 seconds.
///
/// Without this, a shared device would stay locked to a dead client indefinitely: a mouse or keyboard
/// stranded on a machine that is no longer there.
#[cfg(unix)]
pub fn enable_keepalive<S: std::os::fd::AsFd>(stream: &S) -> io::Result<()> {
    SockRef::from(stream).set_tcp_keepalive(&keepalive())
}

#[cfg(windows)]
pub fn enable_keepalive<S: std::os::windows::io::AsSocket>(stream: &S) -> io::Result<()> {
    SockRef::from(stream).set_tcp_keepalive(&keepalive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn keepalive_can_be_enabled_and_is_reported_on() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        enable_keepalive(&c).unwrap();
        assert!(SockRef::from(&c).keepalive().unwrap(), "SO_KEEPALIVE must be on");
        #[cfg(target_os = "linux")]
        {
            assert_eq!(SockRef::from(&c).tcp_keepalive_time().unwrap(), KEEPALIVE_IDLE);
            assert_eq!(SockRef::from(&c).tcp_keepalive_interval().unwrap(), KEEPALIVE_INTERVAL);
            assert_eq!(SockRef::from(&c).tcp_keepalive_retries().unwrap(), 3);
        }
    }
}
