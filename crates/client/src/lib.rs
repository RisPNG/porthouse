use std::time::Duration;

pub mod service;
pub mod usbip_tool;
#[cfg(target_os = "linux")]
pub mod vhci;

use anyhow::{bail, Context, Result};
use porthouse_common::identity::{self, Identity};
use porthouse_common::paths;
use porthouse_common::tls::{client_config, peer_fingerprint, server_name, session_binding};
use porthouse_common::trust::{self, Role, TrustStore};
use porthouse_proto::{decode_frame, encode_frame, AttachInfo, ClientMsg, ServerMsg, DEFAULT_PORT, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

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
    pub async fn open(ctx: &Ctx, addr: &str, require_known: bool) -> Result<Conn> {
        let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .with_context(|| format!("timed out connecting to {addr}"))?
            .with_context(|| format!("connecting to {addr}"))?;
        tcp.set_nodelay(true)?;
        let _ = porthouse_common::net::enable_keepalive(&tcp);
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
            bail!("{addr} is not a paired server (fingerprint {}). Pair it first", &server_fp[..16]);
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

pub fn whoami() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "porthouse-client".into())
}

pub fn with_default_port(h: String) -> String {
    if h.contains(':') {
        h
    } else {
        format!("{h}:{DEFAULT_PORT}")
    }
}

pub async fn pair(ctx: &Ctx, addr: &str, pin_text: &str) -> Result<String> {
    let mut c = Conn::open(ctx, addr, false).await?;
    let hello = c.hello().await?;
    if hello.authorized && ctx.known.contains(&c.server_fp) {
        return Ok(hello.name);
    }
    let pin = trust::normalize_pin(pin_text).context("the PIN is 8 digits, like 1234-5678")?;
    let proof = trust::proof(&pin, &c.binding, Role::Client, &ctx.id.fingerprint);
    match c.call(&ClientMsg::Pair { proof }).await? {
        ServerMsg::Paired { proof } => {
            // The server must prove it knows the PIN too, over this same TLS session. Without
            // this check, an impersonator could simply claim success and be trusted forever.
            if !trust::verify(&pin, &c.binding, Role::Server, &c.server_fp, &proof) {
                bail!("the server did not prove it knows the PIN. NOT trusting it (possible impersonation)");
            }
            ctx.known.add(&c.server_fp, &hello.name)?;
            Ok(hello.name)
        }
        ServerMsg::Error { message, .. } => bail!("pairing failed: {message}"),
        other => bail!("unexpected reply to pairing: {other:?}"),
    }
}

enum Detach {
    #[cfg(target_os = "linux")]
    Vhci(u32),
    Tool {
        exe: std::path::PathBuf,
        port: u32,
    },
}

pub struct Established {
    pub info: AttachInfo,
    pub server_name: String,
    pub addr: String,
    pub busid: String,
    pub port: u32,
    remote: TlsStream<TcpStream>,
    local: TcpStream,
    detach: Detach,
}

async fn negotiate(ctx: &Ctx, addr: &str, busid: &str) -> Result<(ServerHello, AttachInfo, TlsStream<TcpStream>)> {
    let mut c = Conn::open(ctx, addr, true).await?;
    let hello = c.hello().await?;
    if !hello.authorized {
        bail!("this client is no longer authorized on the server; pair it again");
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

#[cfg(not(target_os = "linux"))]
pub async fn establish(ctx: &Ctx, addr: &str, busid: &str) -> Result<Established> {
    // Look for the importer before claiming anything on the server.
    let exe = usbip_tool::find_tool()?;
    establish_with_tool(ctx, addr, busid, exe).await
}

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
