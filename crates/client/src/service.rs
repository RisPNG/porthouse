//! The client service: finds servers, keeps attachments alive, and answers the control channel.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use netusb_common::paths;
use netusb_ipc::{params, Request, Service};
use netusb_proto::{ClientMsg, DeviceInfo, ServerMsg, MDNS_SERVICE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::{establish, establish_with_tool, pair, with_default_port, Conn, Ctx};

const REFRESH: Duration = Duration::from_secs(3);
const MAX_EVENTS: usize = 40;
const SERVERS_FILE: &str = "servers";

#[derive(Serialize, Clone)]
struct ServerView {
    addr: String,
    name: String,
    server_id: String,
    fingerprint: String,
    paired: bool,
    error: Option<String>,
    devices: Vec<DeviceInfo>,
}

#[derive(Serialize, Clone)]
struct AttachmentView {
    id: String,
    addr: String,
    busid: String,
    server_name: String,
    vendor_id: u16,
    product_id: u16,
    port: u32,
    since: u64,
}

struct Attachment {
    view: AttachmentView,
    stop: Option<oneshot::Sender<()>>,
}

struct Shared {
    ctx: Ctx,
    importer: Option<std::path::PathBuf>,
    discovered: Mutex<BTreeSet<String>>,
    manual: Mutex<BTreeSet<String>>,
    servers: Mutex<Vec<ServerView>>,
    attachments: Mutex<HashMap<String, Attachment>>,
    events: Mutex<VecDeque<String>>,
}

type App = Arc<Shared>;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Shared {
    fn event(&self, msg: impl Into<String>) {
        let mut e = self.events.lock().unwrap();
        e.push_back(format!("{}\t{}", now(), msg.into()));
        while e.len() > MAX_EVENTS {
            e.pop_front();
        }
    }

    fn save_manual(&self) {
        let list: Vec<String> = self.manual.lock().unwrap().iter().cloned().collect();
        if let Ok(dir) = paths::service_state_dir() {
            let _ = netusb_common::identity::write_private(&dir.join(SERVERS_FILE), list.join("\n").as_bytes());
        }
    }
}

fn load_manual() -> BTreeSet<String> {
    paths::service_state_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(SERVERS_FILE)).ok())
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn clean_addr(raw: &str) -> Result<String, String> {
    let a = with_default_port(raw.trim().to_string());
    if a.len() > 255 || a.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("invalid address".into());
    }
    Ok(a)
}

#[derive(Deserialize)]
struct AddrParams {
    addr: String,
}

#[derive(Deserialize)]
struct PairParams {
    addr: String,
    pin: String,
}

#[derive(Deserialize)]
struct UseParams {
    addr: String,
    busid: String,
    seconds: Option<u64>,
}

#[derive(Deserialize)]
struct ReleaseParams {
    id: String,
}

#[derive(Deserialize)]
struct ForgetParams {
    fingerprint: String,
}

async fn handle(app: App, req: Request) -> Result<Value, String> {
    match req.method.as_str() {
        "state" => Ok(state(&app)),
        "add_server" => {
            let addr = clean_addr(&params::<AddrParams>(&req)?.addr)?;
            app.manual.lock().unwrap().insert(addr.clone());
            app.save_manual();
            app.event(format!("added server {addr}"));
            Ok(json!({}))
        }
        "remove_server" => {
            let addr = clean_addr(&params::<AddrParams>(&req)?.addr)?;
            app.manual.lock().unwrap().remove(&addr);
            app.save_manual();
            app.event(format!("removed server {addr}"));
            Ok(json!({}))
        }
        "pair" => {
            let p: PairParams = params(&req)?;
            let addr = clean_addr(&p.addr)?;
            let name = pair(&app.ctx, &addr, &p.pin).await.map_err(|e| format!("{e:#}"))?;
            app.event(format!("paired with {name} ({addr})"));
            Ok(json!({ "name": name }))
        }
        "forget" => {
            let want = params::<ForgetParams>(&req)?.fingerprint;
            let matches: Vec<_> = app.ctx.known.entries().into_iter().filter(|(f, _)| f.starts_with(&want)).collect();
            match matches.as_slice() {
                [(f, n)] => {
                    app.ctx.known.remove(f).map_err(|e| format!("{e:#}"))?;
                    app.event(format!("no longer trusting {n}"));
                    Ok(json!({}))
                }
                [] => Err(format!("no paired server matches {want:?}")),
                _ => Err(format!("{want:?} matches several servers; use more characters")),
            }
        }
        "use" => use_device(app, params(&req)?).await,
        "release" => {
            let id = params::<ReleaseParams>(&req)?.id;
            let stop = app.attachments.lock().unwrap().get_mut(&id).and_then(|a| a.stop.take());
            match stop {
                Some(tx) => {
                    let _ = tx.send(());
                    Ok(json!({}))
                }
                None => Err("not attached".into()),
            }
        }
        other => Err(format!("unknown method {other}")),
    }
}

fn state(app: &Shared) -> Value {
    let servers = app.servers.lock().unwrap().clone();
    let mut attachments: Vec<AttachmentView> = app.attachments.lock().unwrap().values().map(|a| a.view.clone()).collect();
    attachments.sort_by(|a, b| a.id.cmp(&b.id));
    let events: Vec<String> = app.events.lock().unwrap().iter().cloned().collect();
    let known: Vec<Value> = app
        .ctx
        .known
        .entries()
        .into_iter()
        .map(|(fingerprint, name)| json!({ "fingerprint": fingerprint, "name": name }))
        .collect();
    let manual: Vec<String> = app.manual.lock().unwrap().iter().cloned().collect();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "servers": servers,
        "attachments": attachments,
        "events": events,
        "known": known,
        "manual": manual,
    })
}

async fn use_device(app: App, r: UseParams) -> Result<Value, String> {
    let addr = clean_addr(&r.addr)?;
    let id = format!("{addr}|{}", r.busid);
    if app.attachments.lock().unwrap().contains_key(&id) {
        return Err("already attached".into());
    }
    let est = match &app.importer {
        Some(exe) => establish_with_tool(&app.ctx, &addr, &r.busid, exe.clone()).await,
        None => establish(&app.ctx, &addr, &r.busid).await,
    }
    .map_err(|e| format!("{e:#}"))?;
    let (tx, rx) = oneshot::channel::<()>();
    let view = AttachmentView {
        id: id.clone(),
        addr: addr.clone(),
        busid: r.busid.clone(),
        server_name: est.server_name.clone(),
        vendor_id: est.info.vendor_id,
        product_id: est.info.product_id,
        port: est.port,
        since: now(),
    };
    app.event(format!(
        "attached {} {:04x}:{:04x} from {}",
        r.busid, view.vendor_id, view.product_id, view.server_name
    ));
    app.attachments
        .lock()
        .unwrap()
        .insert(id.clone(), Attachment { view, stop: Some(tx) });
    let app2 = app.clone();
    let seconds = r.seconds;
    tokio::spawn(async move {
        let limit = async move {
            match seconds {
                Some(s) => tokio::time::sleep(Duration::from_secs(s)).await,
                None => std::future::pending::<()>().await,
            }
        };
        let why = est
            .relay(async move {
                tokio::select! {
                    _ = rx => {}
                    _ = limit => {}
                }
            })
            .await;
        app2.attachments.lock().unwrap().remove(&id);
        app2.event(format!("{id}: {why}"));
    });
    Ok(json!({}))
}

// ---- background work -------------------------------------------------------------------------

async fn probe(ctx: &Ctx, addr: &str) -> ServerView {
    let mut view = ServerView {
        addr: addr.to_string(),
        name: addr.to_string(),
        server_id: addr.to_string(),
        fingerprint: String::new(),
        paired: false,
        error: None,
        devices: Vec::new(),
    };
    let r: Result<()> = async {
        let mut c = Conn::open(ctx, addr, false).await?;
        let h = c.hello().await?;
        view.name = h.name;
        view.server_id = h.id;
        view.fingerprint = c.server_fp.clone();
        view.paired = h.authorized && ctx.known.contains(&c.server_fp);
        if view.paired {
            match c.call(&ClientMsg::ListDevices).await? {
                ServerMsg::Devices(d) => view.devices = d,
                ServerMsg::Error { message, .. } => anyhow::bail!("{message}"),
                other => anyhow::bail!("unexpected reply: {other:?}"),
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = r {
        view.error = Some(format!("{e:#}"));
    }
    view
}

async fn refresh_loop(app: App) {
    loop {
        let addrs: BTreeSet<String> = app
            .discovered
            .lock()
            .unwrap()
            .iter()
            .chain(app.manual.lock().unwrap().iter())
            .cloned()
            .collect();
        let mut set = tokio::task::JoinSet::new();
        for a in addrs {
            let app = app.clone();
            set.spawn(async move { probe(&app.ctx, &a).await });
        }
        let mut views = Vec::new();
        while let Some(Ok(v)) = set.join_next().await {
            views.push(v);
        }
        // One server can be announced on several addresses; keep the first of each identity.
        views.sort_by(|a, b| a.addr.cmp(&b.addr));
        let mut seen = BTreeSet::new();
        views.retain(|v| v.error.is_some() || seen.insert(v.server_id.clone()));
        *app.servers.lock().unwrap() = views;
        tokio::time::sleep(REFRESH).await;
    }
}

fn spawn_mdns(app: App) {
    std::thread::spawn(move || {
        let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
            app.event("mDNS discovery unavailable; add servers by address");
            return;
        };
        let Ok(rx) = daemon.browse(MDNS_SERVICE) else { return };
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        while let Ok(ev) = rx.recv() {
            match ev {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let addrs: Vec<String> = info
                        .get_addresses_v4()
                        .into_iter()
                        .map(|ip| format!("{}:{}", ip, info.get_port()))
                        .collect();
                    let mut d = app.discovered.lock().unwrap();
                    for a in &addrs {
                        d.insert(a.clone());
                    }
                    by_name.insert(info.get_fullname().to_string(), addrs);
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let Some(addrs) = by_name.remove(&fullname) {
                        let mut d = app.discovered.lock().unwrap();
                        for a in addrs {
                            d.remove(&a);
                        }
                    }
                }
                _ => {}
            }
        }
    });
}

/// Run the service until `shutdown` completes, then give every attached device back.
pub async fn run(ctx: Ctx, importer: Option<std::path::PathBuf>, shutdown: impl Future<Output = ()>) -> Result<()> {
    let app: App = Arc::new(Shared {
        ctx,
        importer,
        discovered: Mutex::default(),
        manual: Mutex::new(load_manual()),
        servers: Mutex::default(),
        attachments: Mutex::default(),
        events: Mutex::default(),
    });
    spawn_mdns(app.clone());
    tokio::spawn(refresh_loop(app.clone()));

    let handler_app = app.clone();
    let api = netusb_ipc::serve(Service::Client, move |req| handle(handler_app.clone(), req));
    tokio::select! {
        r = api => r?,
        _ = shutdown => {}
    }

    let stops: Vec<_> = app.attachments.lock().unwrap().values_mut().filter_map(|a| a.stop.take()).collect();
    for s in stops {
        let _ = s.send(());
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}
