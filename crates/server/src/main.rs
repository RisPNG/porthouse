use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod emulated;
mod engine;

use anyhow::{bail, Context, Result};
use nusb::MaybeFuture;
use porthouse_common::tls::{peer_fingerprint, server_config, session_binding};
use porthouse_common::trust::{self, PinFile, Role, TrustStore};
use porthouse_common::{identity, paths};
use porthouse_proto::{
    decode_frame, encode_frame, AttachInfo, ClientMsg, DeviceInfo, ServerMsg, Speed, DEFAULT_PORT, MDNS_SERVICE, PROTO_VERSION,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

struct InUse<'a> {
    srv: &'a Server,
    busid: String,
}

impl Drop for InUse<'_> {
    fn drop(&mut self) {
        self.srv.in_use.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.busid);
    }
}

struct Active<'a>(&'a AtomicUsize);

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PORTHOUSE_TRACE").is_some())
}

struct Server {
    id: String,
    name: String,
    in_use: Mutex<HashMap<String, String>>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    active: AtomicUsize,
    acceptor: TlsAcceptor,
    fingerprint: String,
    authorized: TrustStore,
    pin: PinFile,
    test_devices: Vec<emulated::Kind>,
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "porthouse-host".into())
}

fn server_id() -> String {
    std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(hostname)
}

fn map_speed(s: Option<nusb::Speed>) -> Speed {
    match s {
        Some(nusb::Speed::Low) => Speed::Low,
        Some(nusb::Speed::Full) => Speed::Full,
        Some(nusb::Speed::High) => Speed::High,
        Some(nusb::Speed::Super) => Speed::Super,
        Some(nusb::Speed::SuperPlus) => Speed::SuperPlus,
        _ => Speed::Unknown,
    }
}

fn busid_of(d: &nusb::DeviceInfo) -> String {
    let chain = d.port_chain().iter().map(|p| p.to_string()).collect::<Vec<_>>().join(".");
    #[cfg(target_os = "linux")]
    let bus = d.busnum().to_string();
    #[cfg(not(target_os = "linux"))]
    let bus = d.bus_id().to_string();
    format!("{bus}-{chain}")
}

fn raw_devices() -> Result<Vec<nusb::DeviceInfo>> {
    let mut v: Vec<_> = nusb::list_devices()
        .wait()
        .context("enumerating USB devices")?
        .filter(|d| d.class() != 9)
        .collect();
    v.sort_by_key(busid_of);
    Ok(v)
}

fn list_devices(srv: &Server) -> Result<Vec<DeviceInfo>> {
    let in_use = srv.in_use.lock().unwrap();
    let mut list: Vec<DeviceInfo> = raw_devices()?
        .into_iter()
        .map(|d| {
            let busid = busid_of(&d);
            DeviceInfo {
                in_use_by: in_use.get(&busid).cloned(),
                busid,
                vendor_id: d.vendor_id(),
                product_id: d.product_id(),
                class: d.class(),
                speed: map_speed(d.speed()),
                manufacturer: d.manufacturer_string().map(str::to_string),
                product: d.product_string().map(str::to_string),
                serial: d.serial_number().map(str::to_string),
                input: d.interfaces().any(|i| i.class() == 3), // USB HID: mice, keyboards, game controllers
            }
        })
        .collect();
    for kind in &srv.test_devices {
        list.push(DeviceInfo {
            busid: kind.busid().into(),
            vendor_id: emulated::VENDOR_ID,
            product_id: kind.product_id(),
            class: 0,
            speed: kind.speed_class(),
            manufacturer: Some("porthouse".into()),
            product: Some(format!("{} (test device)", kind.product_name())),
            serial: Some(kind.serial().into()),
            in_use_by: in_use.get(kind.busid()).cloned(),
            input: kind.is_input(),
        });
    }
    Ok(list)
}

fn usb_speed_number(s: Option<nusb::Speed>) -> u32 {
    match s {
        Some(nusb::Speed::Low) => 1,
        Some(nusb::Speed::Full) => 2,
        Some(nusb::Speed::High) => 3,
        Some(nusb::Speed::Super) => 5,
        Some(nusb::Speed::SuperPlus) => 6,
        _ => 0,
    }
}

fn attach_info(d: &nusb::DeviceInfo) -> AttachInfo {
    #[cfg(target_os = "linux")]
    let busnum = d.busnum() as u32;
    #[cfg(not(target_os = "linux"))]
    let busnum = 1;
    AttachInfo {
        busid: busid_of(d),
        busnum,
        devnum: d.device_address() as u32,
        speed: usb_speed_number(d.speed()),
        vendor_id: d.vendor_id(),
        product_id: d.product_id(),
        bcd_device: d.device_version(),
        class: d.class(),
        subclass: d.subclass(),
        protocol: d.protocol(),
        configuration_value: 1,
        num_configurations: 1,
        num_interfaces: d.interfaces().count().min(255) as u8,
    }
}

async fn send<W: AsyncWrite + Unpin>(sock: &mut W, msg: &ServerMsg) -> Result<()> {
    sock.write_all(&encode_frame(msg)?).await?;
    Ok(())
}

fn err(code: u16, message: impl Into<String>) -> ServerMsg {
    ServerMsg::Error {
        code,
        message: message.into(),
    }
}

async fn serve(tcp: TcpStream, srv: Arc<Server>) -> Result<()> {
    let tls = tokio::time::timeout(Duration::from_secs(10), srv.acceptor.accept(tcp))
        .await
        .context("TLS handshake timed out")?
        .context("TLS handshake failed")?;
    let (peer_fp, binding) = {
        let (_, conn) = tls.get_ref();
        (
            peer_fingerprint(conn).context("client presented no certificate")?,
            session_binding(conn)?,
        )
    };
    session(tls, peer_fp, binding, srv).await
}

async fn session<S>(mut sock: S, peer_fp: String, binding: [u8; 32], srv: Arc<Server>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut buf = Vec::<u8>::new();
    let mut tmp = [0u8; 8192];
    let mut greeted = false;
    let mut client_name = String::from("unknown");
    let mut authorized = srv.authorized.contains(&peer_fp);
    loop {
        while let Some((msg, used)) = decode_frame::<ClientMsg>(&buf)? {
            buf.drain(..used);
            match msg {
                ClientMsg::Hello { proto, client_name: name } => {
                    if proto != PROTO_VERSION {
                        send(
                            &mut sock,
                            &err(1, format!("unsupported protocol {proto}, this server speaks {PROTO_VERSION}")),
                        )
                        .await?;
                        return Ok(());
                    }
                    // Prefer the name recorded at pairing; the client's own claim is display-only.
                    client_name = srv.authorized.name_of(&peer_fp).unwrap_or(name);
                    greeted = true;
                    send(
                        &mut sock,
                        &ServerMsg::Hello {
                            proto: PROTO_VERSION,
                            server_id: srv.id.clone(),
                            server_name: srv.name.clone(),
                            version: env!("CARGO_PKG_VERSION").into(),
                            authorized,
                        },
                    )
                    .await?;
                }
                _ if !greeted => {
                    send(&mut sock, &err(2, "hello required first")).await?;
                    return Ok(());
                }
                ClientMsg::Pair { proof } => {
                    if authorized {
                        send(&mut sock, &err(10, "this client is already paired")).await?;
                        continue;
                    }
                    let Some(pin) = srv.pin.active() else {
                        send(&mut sock, &err(7, "pairing is not open; run `porthouse-server pair` on the server")).await?;
                        return Ok(());
                    };
                    if !trust::verify(&pin, &binding, Role::Client, &peer_fp, &proof) {
                        srv.pin.record_failure();
                        eprintln!("pairing attempt with a wrong PIN from {client_name:?}");
                        send(&mut sock, &err(8, "wrong PIN")).await?;
                        return Ok(());
                    }
                    srv.pin.consume();
                    srv.authorized.add(&peer_fp, &client_name)?;
                    authorized = true;
                    eprintln!("paired new client {client_name:?} ({})", &peer_fp[..16]);
                    let proof = trust::proof(&pin, &binding, Role::Server, &srv.fingerprint);
                    send(&mut sock, &ServerMsg::Paired { proof }).await?;
                }
                _ if !authorized => {
                    send(&mut sock, &err(9, "this client is not paired with the server")).await?;
                    return Ok(());
                }
                ClientMsg::ListDevices => {
                    let reply = match list_devices(&srv) {
                        Ok(d) => ServerMsg::Devices(d),
                        Err(e) => err(3, format!("{e:#}")),
                    };
                    send(&mut sock, &reply).await?;
                }
                ClientMsg::Ping(n) => send(&mut sock, &ServerMsg::Pong(n)).await?,
                ClientMsg::Attach { busid } => {
                    let virtual_kind = emulated::Kind::from_busid(&busid).filter(|k| srv.test_devices.contains(k));
                    let is_virtual = virtual_kind.is_some();
                    let found = if is_virtual {
                        None
                    } else {
                        raw_devices()?.into_iter().find(|d| busid_of(d) == busid)
                    };
                    if found.is_none() && !is_virtual {
                        send(&mut sock, &err(4, format!("no such device {busid}"))).await?;
                        continue;
                    }
                    if !buf.is_empty() {
                        send(&mut sock, &err(5, "unexpected data after attach")).await?;
                        return Ok(());
                    }
                    let claim: Result<(), String> = {
                        let mut in_use = srv.in_use.lock().unwrap();
                        match in_use.get(&busid) {
                            Some(by) => Err(format!("device {busid} is already in use by {by}")),
                            None => {
                                in_use.insert(busid.clone(), client_name.clone());
                                Ok(())
                            }
                        }
                    };
                    if let Err(message) = claim {
                        send(&mut sock, &err(6, message)).await?;
                        continue;
                    }
                    let _guard = InUse {
                        srv: &srv,
                        busid: busid.clone(),
                    };
                    srv.active.fetch_add(1, Ordering::SeqCst);
                    let _active = Active(&srv.active);
                    let info = match (&found, virtual_kind) {
                        (Some(dev), _) => attach_info(dev),
                        (None, Some(kind)) => kind.attach_info(),
                        (None, None) => unreachable!("a device was found or a virtual kind matched above"),
                    };
                    // Take the device first: if that fails, say so now instead of reporting success.
                    let prepared = match found {
                        Some(dev) => match engine::prepare(dev, busid.clone()).await {
                            Ok(p) => Some(p),
                            Err(e) => {
                                eprintln!("cannot attach {busid} for {client_name:?}: {e:#}");
                                send(&mut sock, &err(11, format!("{e:#}"))).await?;
                                continue;
                            }
                        },
                        None => None,
                    };
                    send(&mut sock, &ServerMsg::Attached(info)).await?;
                    eprintln!("{client_name:?} attached {busid}");
                    let result = match (prepared, virtual_kind) {
                        (Some(p), _) => engine::serve(p, sock, srv.shutdown.clone()).await,
                        (None, Some(kind)) => emulated::run(kind, sock, srv.shutdown.clone()).await,
                        (None, None) => unreachable!("a device was found or a virtual kind matched above"),
                    };
                    eprintln!("{client_name:?} released {busid}");
                    return result;
                }
            }
        }
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

fn advertise(name: &str, id: &str, fp: &str, port: u16) -> Result<mdns_sd::ServiceDaemon> {
    let daemon = mdns_sd::ServiceDaemon::new().context("starting mDNS daemon")?;
    let host = format!("{}.local.", hostname());
    let ver = PROTO_VERSION.to_string();
    let props = [("id", id), ("v", ver.as_str()), ("fp", &fp[..16])];
    let info = mdns_sd::ServiceInfo::new(MDNS_SERVICE, name, &host, "", port, &props[..])
        .context("building mDNS record")?
        .enable_addr_auto();
    daemon.register(info).context("registering mDNS service")?;
    Ok(daemon)
}

fn usage() {
    println!(
        "porthouse-server [run] [--port N] [--name NAME] [--no-mdns] [--test-device mouse|loopback]...   run the server
                                                           (--test-device adds software test devices; the mouse moves the pointer of whoever attaches it)
porthouse-server pair                                         open a 10 minute pairing window and print its PIN
porthouse-server clients                                      list paired clients
porthouse-server revoke FINGERPRINT                           remove a paired client

pair, clients and revoke talk to the running service."
    );
}

async fn control(method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let mut c = porthouse_ipc::Client::connect(porthouse_ipc::Service::Server).await?;
    c.call(method, params).await
}

fn text<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

struct Handler {
    srv: Arc<Server>,
    port: u16,
}

async fn handle(h: Arc<Handler>, req: porthouse_ipc::Request) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let srv = &h.srv;
    match req.method.as_str() {
        "status" => {
            let devices = list_devices(srv).map_err(|e| format!("{e:#}"))?;
            let clients: Vec<_> = srv
                .authorized
                .entries()
                .into_iter()
                .map(|(fingerprint, name)| json!({ "fingerprint": fingerprint, "name": name }))
                .collect();
            let pairing = srv.pin.window().map(|(pin, expires)| json!({ "pin": pin, "expires": expires }));
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "name": srv.name,
                "id": srv.id,
                "fingerprint": srv.fingerprint,
                "port": h.port,
                "devices": devices,
                "clients": clients,
                "pairing": pairing,
            }))
        }
        "open_pairing" => {
            let (pin, expires) = srv.pin.open_window().map_err(|e| format!("{e:#}"))?;
            Ok(json!({ "pin": pin, "expires": expires }))
        }
        "revoke" => {
            let fp = req.params["fingerprint"].as_str().ok_or("fingerprint is required")?;
            let matches: Vec<_> = srv.authorized.entries().into_iter().filter(|(f, _)| f.starts_with(fp)).collect();
            match matches.as_slice() {
                [(f, n)] => {
                    srv.authorized.remove(f).map_err(|e| format!("{e:#}"))?;
                    Ok(json!({ "name": n }))
                }
                [] => Err(format!("no paired client matches {fp:?}")),
                _ => Err(format!("{fp:?} matches several clients; use more characters")),
            }
        }
        other => Err(format!("unknown method {other}")),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pair") => {
            let r = control("open_pairing", serde_json::Value::Null).await?;
            let status = control("status", serde_json::Value::Null).await?;
            println!(
                "Pairing is open for 10 minutes. On the client run:\n\n    porthouse-client pair <this-host>[:{DEFAULT_PORT}]\n\nand enter this PIN:  {}\n\nServer fingerprint: {}",
                text(&r, "pin"),
                text(&status, "fingerprint")
            );
            return Ok(());
        }
        Some("clients") => {
            let status = control("status", serde_json::Value::Null).await?;
            let all = status["clients"].as_array().cloned().unwrap_or_default();
            if all.is_empty() {
                println!("no paired clients");
            }
            for c in all {
                println!("{}  {}", text(&c, "fingerprint"), text(&c, "name"));
            }
            return Ok(());
        }
        Some("revoke") => {
            let fp = args.get(1).context("usage: porthouse-server revoke FINGERPRINT")?;
            let r = control("revoke", serde_json::json!({ "fingerprint": fp })).await?;
            println!("revoked {}", text(&r, "name"));
            return Ok(());
        }
        Some("-V") | Some("--version") => {
            println!("porthouse-server {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h") | Some("--help") => {
            usage();
            return Ok(());
        }
        #[cfg(windows)]
        Some("service") => return windows_service_main(),
        #[cfg(windows)]
        Some("install-service") => {
            porthouse_common::winsvc::install(
                SERVICE_NAME,
                "Porthouse server",
                "Shares this computer's USB devices with paired computers.",
                &[],
            )?;
            println!("service {SERVICE_NAME} installed and started");
            return Ok(());
        }
        #[cfg(windows)]
        Some("uninstall-service") => {
            porthouse_common::winsvc::uninstall(SERVICE_NAME)?;
            println!("service {SERVICE_NAME} removed");
            return Ok(());
        }
        Some("run") => {
            args.remove(0);
        }
        _ => {}
    }

    let opts = parse_options(args)?;
    serve_until(opts, porthouse_common::shutdown::console_signal()).await
}

#[cfg(windows)]
const SERVICE_NAME: &str = "porthouse-server";

#[cfg(windows)]
fn windows_service_main() -> Result<()> {
    if let Ok(dir) = paths::state_dir() {
        let _ = porthouse_common::winsvc::log_to(&dir.join("server.log"));
    }
    porthouse_common::winsvc::dispatch(SERVICE_NAME, |stop| {
        let opts = parse_options(Vec::new())?;
        tokio::runtime::Runtime::new()?.block_on(serve_until(opts, async {
            let _ = stop.await;
        }))
    })
}

struct Options {
    port: u16,
    name: String,
    mdns: bool,
    test_devices: Vec<emulated::Kind>,
}

fn parse_options(args: Vec<String>) -> Result<Options> {
    let mut port = DEFAULT_PORT;
    let mut name = hostname();
    let mut mdns = true;
    let mut test_devices: Vec<emulated::Kind> = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => port = it.next().context("--port needs a value")?.parse()?,
            "--name" => name = it.next().context("--name needs a value")?,
            "--no-mdns" => mdns = false,
            "--test-device" => {
                let v = it.next().context("--test-device needs `mouse` or `loopback`")?;
                let kind =
                    emulated::Kind::parse(&v).with_context(|| format!("--test-device supports `mouse` and `loopback`, not {v:?}"))?;
                if !test_devices.contains(&kind) {
                    test_devices.push(kind);
                }
            }
            other => bail!("unknown argument {other:?} (see --help)"),
        }
    }
    Ok(Options {
        port,
        name,
        mdns,
        test_devices,
    })
}

async fn serve_until(opts: Options, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
    let Options {
        port,
        name,
        mdns,
        test_devices,
    } = opts;
    let state = paths::state_dir().context("preparing the state directory")?;
    let id = identity::load_or_create(&state, "server")?;
    let acceptor = TlsAcceptor::from(server_config(&id)?);
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    engine::restore_stale(busid_of).await;
    let srv = Arc::new(Server {
        id: server_id(),
        name,
        in_use: Mutex::new(HashMap::new()),
        shutdown: stop_rx,
        active: AtomicUsize::new(0),
        acceptor,
        fingerprint: id.fingerprint.clone(),
        authorized: TrustStore::new(&state, "authorized_clients"),
        pin: PinFile::new(&state),
        test_devices,
    });

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("binding port {port}"))?;
    eprintln!(
        "porthouse-server {} listening on :{port} as {:?}",
        env!("CARGO_PKG_VERSION"),
        srv.name
    );
    eprintln!("fingerprint {}", srv.fingerprint);
    if srv.authorized.entries().is_empty() {
        eprintln!("no clients are paired yet; open a pairing window with `porthouse-server pair` or from the Porthouse program");
    }

    let control_srv = srv.clone();
    let handler = Arc::new(Handler { srv: control_srv, port });
    tokio::spawn(async move {
        if let Err(e) = porthouse_ipc::serve(porthouse_ipc::Service::Server, move |req| handle(handler.clone(), req)).await {
            eprintln!("control endpoint stopped: {e:#}");
        }
    });

    let _mdns = if mdns {
        match advertise(&srv.name, &srv.id, &srv.fingerprint, port) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("warning: mDNS advertisement disabled: {e:#}");
                None
            }
        }
    } else {
        None
    };

    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (sock, peer) = accepted?;
                let _ = sock.set_nodelay(true);
                let _ = porthouse_common::net::enable_keepalive(&sock);
                let srv = srv.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(sock, srv).await {
                        eprintln!("connection {peer}: {e:#}");
                    }
                });
            }
            _ = &mut shutdown => break,
        }
    }

    eprintln!("shutting down, releasing devices...");
    let _ = stop_tx.send(true);
    for _ in 0..100 {
        if srv.active.load(Ordering::SeqCst) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
