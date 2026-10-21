use std::time::Duration;

pub mod daemon;
pub mod usbip_tool;
#[cfg(target_os = "linux")]
pub mod vhci;

use anyhow::{bail, Context, Result};
use netusb_common::identity::{self, Identity};
use netusb_common::paths;
use netusb_common::tls::{client_config, peer_fingerprint, server_name, session_binding};
use netusb_common::trust::{self, Role, TrustStore};
use netusb_proto::{decode_frame, encode_frame, AttachInfo, ClientMsg, DeviceInfo, ServerMsg, DEFAULT_PORT, MDNS_SERVICE, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

/// Everything a command needs: our identity and the servers we have chosen to trust.
pub struct Ctx {
    pub id: Identity,
    pub connector: TlsConnector,
    pub known: TrustStore,
}

impl Ctx {
    pub fn load() -> Result<Ctx> {
        let dir = paths::state_dir().context("preparing the state directory")?;
        let id = identity::load_or_create(&dir, "client")?;
        let connector = TlsConnector::from(client_config(&id)?);
        Ok(Ctx {
            id,
            connector,
            known: TrustStore::new(&dir, "known_servers"),
        })
    }
}

pub struct Conn {
    pub sock: TlsStream<TcpStream>,
    pub buf: Vec<u8>,
    pub server_fp: String,
    pub binding: [u8; 32],
}

pub struct ServerHello {
    pub name: String,
    pub id: String,
    pub authorized: bool,
}

impl Conn {
    /// Open an encrypted connection. With `require_known`, refuse any server whose certificate
    /// fingerprint has not been pinned by a previous pairing.
    pub async fn open(ctx: &Ctx, addr: &str, require_known: bool) -> Result<Conn> {
        let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .with_context(|| format!("timed out connecting to {addr}"))?
            .with_context(|| format!("connecting to {addr}"))?;
        tcp.set_nodelay(true)?;
        let sock = tokio::time::timeout(Duration::from_secs(10), ctx.connector.connect(server_name(), tcp))
            .await
            .context("TLS handshake timed out")?
            .context("TLS handshake failed")?;
        let (server_fp, binding) = {
            let (_, conn) = sock.get_ref();
            (
                peer_fingerprint(conn).context("server presented no certificate")?,
                session_binding(conn)?,
            )
        };
        if require_known && !ctx.known.contains(&server_fp) {
            bail!(
                "{addr} is not a paired server (fingerprint {}). Pair first with: netusb-client pair {addr}",
                &server_fp[..16]
            );
        }
        Ok(Conn {
            sock,
            buf: Vec::new(),
            server_fp,
            binding,
        })
    }

    pub async fn hello(&mut self) -> Result<ServerHello> {
        match self
            .call(&ClientMsg::Hello {
                proto: PROTO_VERSION,
                client_name: whoami(),
            })
            .await?
        {
            ServerMsg::Hello {
                server_name,
                server_id,
                authorized,
                ..
            } => Ok(ServerHello {
                name: server_name,
                id: server_id,
                authorized,
            }),
            ServerMsg::Error { message, .. } => bail!("server refused: {message}"),
            other => bail!("unexpected reply to hello: {other:?}"),
        }
    }

    pub async fn call(&mut self, msg: &ClientMsg) -> Result<ServerMsg> {
        self.sock.write_all(&encode_frame(msg)?).await?;
        let mut tmp = [0u8; 8192];
        loop {
            if let Some((reply, used)) = decode_frame::<ServerMsg>(&self.buf)? {
                self.buf.drain(..used);
                return Ok(reply);
            }
            let n = tokio::time::timeout(Duration::from_secs(5), self.sock.read(&mut tmp))
                .await
                .context("timed out waiting for the server")??;
            if n == 0 {
                bail!("server closed the connection");
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }
}

pub struct Listing {
    pub server_name: String,
    pub server_id: String,
    pub addr: String,
    pub devices: Vec<DeviceInfo>,
}

pub async fn query(ctx: &Ctx, addr: &str) -> Result<Listing> {
    let mut c = Conn::open(ctx, addr, true).await?;
    let hello = c.hello().await?;
    if !hello.authorized {
        bail!("this client is no longer authorized on the server; pair again with: netusb-client pair {addr}");
    }
    let devices = match c.call(&ClientMsg::ListDevices).await? {
        ServerMsg::Devices(d) => d,
        ServerMsg::Error { message, .. } => bail!("server error: {message}"),
        other => bail!("unexpected reply to list: {other:?}"),
    };
    Ok(Listing {
        server_name: hello.name,
        server_id: hello.id,
        addr: addr.to_string(),
        devices,
    })
}

pub fn whoami() -> String {
    // Under sudo the invoking user is the more meaningful name.
    std::env::var("SUDO_USER")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

pub fn with_default_port(h: String) -> String {
    if h.contains(':') {
        h
    } else {
        format!("{h}:{DEFAULT_PORT}")
    }
}

/// Browse mDNS for a short window and return every resolved `host:port`.
pub fn discover(window: Duration) -> Result<Vec<String>> {
    let daemon = mdns_sd::ServiceDaemon::new().context("starting mDNS daemon")?;
    let rx = daemon.browse(MDNS_SERVICE).context("browsing mDNS")?;
    let deadline = std::time::Instant::now() + window;
    let mut found = Vec::new();
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                // Prefer IPv4: it is what every platform can route without scope ids.
                if let Some(ip) = info.get_addresses_v4().into_iter().next() {
                    let addr = format!("{}:{}", ip, info.get_port());
                    if !found.contains(&addr) {
                        found.push(addr);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}

/// Establish trust with a server: prove we know the PIN it shows, and check it proves the same.
pub async fn cmd_pair(ctx: &Ctx, addr: &str, pin_arg: Option<String>) -> Result<()> {
    let mut c = Conn::open(ctx, addr, false).await?;
    let hello = c.hello().await?;
    if hello.authorized && ctx.known.contains(&c.server_fp) {
        println!("already paired with {} ({})", hello.name, addr);
        return Ok(());
    }
    println!("server {:?} at {addr}, fingerprint {}", hello.name, &c.server_fp[..16]);

    let raw = match pin_arg {
        Some(p) => p,
        None => {
            eprint!("PIN shown by `netusb-server pair` on that machine: ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line
        }
    };
    let pin = trust::normalize_pin(&raw).context("the PIN is 8 digits, like 1234-5678")?;

    let proof = trust::proof(&pin, &c.binding, Role::Client, &ctx.id.fingerprint);
    match c.call(&ClientMsg::Pair { proof }).await? {
        ServerMsg::Paired { proof } => {
            // The server must prove it knows the PIN too, over this same TLS session. Without
            // this check, an impersonator could simply claim success and be trusted forever.
            if !trust::verify(&pin, &c.binding, Role::Server, &c.server_fp, &proof) {
                bail!("the server did not prove it knows the PIN. NOT trusting it (possible impersonation)");
            }
            ctx.known.add(&c.server_fp, &hello.name)?;
            println!("paired with {:?}. You can now run: netusb-client list", hello.name);
            Ok(())
        }
        ServerMsg::Error { message, .. } => bail!("pairing failed: {message}"),
        other => bail!("unexpected reply to pairing: {other:?}"),
    }
}

/// How to unplug the virtual device again.
enum Detach {
    /// Linux `vhci_hcd` port.
    #[cfg(target_os = "linux")]
    Vhci(u32),
    /// An external USB/IP importer (`usbip.exe`) that owns the port.
    Tool { exe: std::path::PathBuf, port: u32 },
}

/// A device that is attached to this machine's virtual USB bus and waiting to be relayed.
pub struct Established {
    pub info: AttachInfo,
    pub server_name: String,
    pub addr: String,
    pub busid: String,
    /// Virtual port number on the local controller.
    pub port: u32,
    remote: TlsStream<TcpStream>,
    local: TcpStream,
    detach: Detach,
}

/// The shared first half of attaching: authenticate, claim the device on the server, and return
/// what the server told us together with the (now raw) encrypted connection.
async fn negotiate(ctx: &Ctx, addr: &str, busid: &str) -> Result<(ServerHello, AttachInfo, TlsStream<TcpStream>)> {
    let mut c = Conn::open(ctx, addr, true).await?;
    let hello = c.hello().await?;
    if !hello.authorized {
        bail!("this client is no longer authorized on the server; pair again with: netusb-client pair {addr}");
    }
    let info: AttachInfo = match c.call(&ClientMsg::Attach { busid: busid.to_string() }).await? {
        ServerMsg::Attached(i) => i,
        ServerMsg::Error { message, .. } => bail!("server refused attach: {message}"),
        other => bail!("unexpected reply to attach: {other:?}"),
    };
    if !c.buf.is_empty() {
        bail!("server sent unexpected data after attach");
    }
    Ok((hello, info, c.sock))
}

/// Claim `busid` on the server and plug it into the local virtual USB bus.
#[cfg(target_os = "linux")]
pub async fn establish(ctx: &Ctx, addr: &str, busid: &str) -> Result<Established> {
    let (hello, info, remote) = negotiate(ctx, addr, busid).await?;

    // The kernel needs a real socket for its end of the transport. Make a private loopback pair:
    // the kernel gets one side, this process relays the other to the encrypted remote.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let kernel_side = std::net::TcpStream::connect(listener.local_addr()?)?;
    let (relay_side, peer) = listener.accept()?;
    if peer != kernel_side.local_addr()? {
        bail!("another local process connected to the relay socket");
    }
    drop(listener);
    kernel_side.set_nodelay(true)?;
    relay_side.set_nodelay(true)?;
    relay_side.set_nonblocking(true)?;
    let local = TcpStream::from_std(relay_side)?;

    let port = vhci::find_free_port(info.speed)?;
    {
        use std::os::fd::AsRawFd;
        vhci::attach(port, kernel_side.as_raw_fd(), info.devid(), info.speed)?;
    }
    drop(kernel_side); // the kernel holds its own reference now
    Ok(Established {
        info,
        server_name: hello.name,
        addr: addr.to_string(),
        busid: busid.to_string(),
        port,
        remote,
        local,
        detach: Detach::Vhci(port),
    })
}

/// Windows (and anything else without `vhci_hcd`): attach through the `usbip-win2` importer.
#[cfg(not(target_os = "linux"))]
pub async fn establish(ctx: &Ctx, addr: &str, busid: &str) -> Result<Established> {
    // Look for the importer before claiming anything on the server.
    let exe = usbip_tool::find_tool()?;
    establish_with_tool(ctx, addr, busid, exe).await
}

/// Attach using a specific USB/IP importer program (see [`usbip_tool`]). Public so it can be
/// exercised on any platform with a stand-in importer.
pub async fn establish_with_tool(ctx: &Ctx, addr: &str, busid: &str, exe: std::path::PathBuf) -> Result<Established> {
    let (hello, info, remote) = negotiate(ctx, addr, busid).await?;
    // From here the server holds the device for us. If the importer fails, dropping `remote`
    // closes the connection and the server hands the device back.
    let (local, port) = usbip_tool::attach(&exe, &info).await?;
    Ok(Established {
        info,
        server_name: hello.name,
        addr: addr.to_string(),
        busid: busid.to_string(),
        port,
        remote,
        local,
        detach: Detach::Tool { exe, port },
    })
}

impl Established {
    /// Carry USB traffic until the server closes, an error occurs, or `stop` completes. Always
    /// unplugs the virtual device before returning. The returned string says why it ended.
    pub async fn relay(mut self, stop: impl std::future::Future<Output = ()>) -> String {
        let reason = tokio::select! {
            r = tokio::io::copy_bidirectional(&mut self.remote, &mut self.local) => match r {
                Ok(_) => "the server closed the connection".to_string(),
                Err(e) => format!("connection lost: {e}"),
            },
            _ = stop => "released".to_string(),
        };
        match &self.detach {
            #[cfg(target_os = "linux")]
            Detach::Vhci(port) => {
                let _ = vhci::detach(*port);
            }
            Detach::Tool { exe, port } => {
                let _ = usbip_tool::detach(exe, *port).await;
            }
        }
        reason
    }
}

/// Open `url` in the user's default browser. Only ever called with the daemon's own loopback URL.
pub fn open_in_browser(url: &str) -> Result<()> {
    if !url.starts_with("http://127.0.0.1:") {
        bail!("refusing to open a non-local address");
    }
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", "", url]).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().context("could not start the browser opener")?;
    Ok(())
}

/// Open the web interface of the running daemon (its sign-in address is saved by `gui`).
pub fn cmd_open() -> Result<()> {
    let file = paths::state_dir()?.join("ui.url");
    let url = std::fs::read_to_string(&file)
        .with_context(|| format!("no running interface found ({}). Start it with: netusb-client gui", file.display()))?;
    let url = url.trim();
    println!("{url}");
    open_in_browser(url)
}

/// CLI wrapper: attach and hold until Ctrl-C / SIGTERM.
pub async fn cmd_use(ctx: &Ctx, addr: &str, busid: &str, importer: Option<std::path::PathBuf>, seconds: Option<u64>) -> Result<()> {
    let est = match importer {
        Some(exe) => establish_with_tool(ctx, addr, busid, exe).await?,
        None => establish(ctx, addr, busid).await?,
    };
    println!(
        "attached {busid} ({:04x}:{:04x}) from {} on virtual port {}; press Ctrl-C to release",
        est.info.vendor_id, est.info.product_id, est.server_name, est.port
    );
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let limit = async {
        match seconds {
            Some(s) => tokio::time::sleep(std::time::Duration::from_secs(s)).await,
            None => std::future::pending::<()>().await,
        }
    };
    let stop = async {
        #[cfg(unix)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = limit => {}
        }
        #[cfg(not(unix))]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = limit => {}
        }
    };
    let why = est.relay(stop).await;
    eprintln!("{why}");
    Ok(())
}

pub fn print_listing(l: &Listing) {
    println!("{} ({})  [{}]", l.server_name, l.addr, &l.server_id[..l.server_id.len().min(8)]);
    if l.devices.is_empty() {
        println!("   (no shareable devices)");
    }
    for d in &l.devices {
        let name = match (&d.manufacturer, &d.product) {
            (Some(m), Some(p)) => format!("{m} {p}"),
            (None, Some(p)) => p.clone(),
            _ => "(unnamed device)".into(),
        };
        let state = d.in_use_by.as_deref().map(|u| format!("  in use by {u}")).unwrap_or_default();
        println!(
            "   {:<8} {:04x}:{:04x}  {:?}  {}{}",
            d.busid, d.vendor_id, d.product_id, d.speed, name, state
        );
    }
}
