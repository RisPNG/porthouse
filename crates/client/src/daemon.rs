//! Local daemon: discovers servers, keeps attachments alive, and serves the web UI.
//!
//! The UI is bound to loopback only and every request is checked for a secret token (carried in
//! an HttpOnly SameSite=Strict cookie), a loopback `Host` header (defeats DNS rebinding), and, for
//! writes, a JSON content type and a same-origin `Origin` (defeats cross-site requests).

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use netusb_common::{identity::write_private, paths};
use netusb_proto::{ClientMsg, DeviceInfo, ServerMsg, MDNS_SERVICE};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio::sync::oneshot;

use crate::{establish, whoami, with_default_port, Conn, Ctx};

const UI_HTML: &str = include_str!("ui.html");
const REFRESH: Duration = Duration::from_secs(3);
const MAX_EVENTS: usize = 40;

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
    token: String,
    port: u16,
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
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn fail(e: anyhow::Error) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}"))
}

// ---- request guard ---------------------------------------------------------------------------

fn cookie_token(h: &HeaderMap) -> Option<String> {
    let raw = h.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|kv| kv.trim().strip_prefix("netusb=").map(str::to_string))
}

fn token_ok(app: &Shared, given: &str) -> bool {
    given.as_bytes().ct_eq(app.token.as_bytes()).into()
}

fn host_ok(app: &Shared, h: &HeaderMap) -> bool {
    let Some(host) = h.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    host == format!("127.0.0.1:{}", app.port) || host == format!("localhost:{}", app.port)
}

fn origin_ok(app: &Shared, h: &HeaderMap) -> bool {
    match h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        None => true, // non-browser clients (curl) send none; the cookie is still required
        Some(o) => o == format!("http://127.0.0.1:{}", app.port) || o == format!("http://localhost:{}", app.port),
    }
}

async fn guard(State(app): State<App>, req: Request, next: Next) -> Response {
    let h = req.headers();
    if !host_ok(&app, h) {
        return (StatusCode::FORBIDDEN, "bad Host header").into_response();
    }
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();

    // The one unauthenticated entry point: exchange the URL token for a cookie.
    if req.method() == Method::GET && path == "/" {
        if let Some(t) = query.split('&').find_map(|kv| kv.strip_prefix("token=")) {
            if token_ok(&app, t) {
                let mut r = Response::new(axum::body::Body::empty());
                *r.status_mut() = StatusCode::SEE_OTHER;
                r.headers_mut().insert(header::LOCATION, HeaderValue::from_static("/"));
                let c = format!("netusb={}; HttpOnly; SameSite=Strict; Path=/", app.token);
                r.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&c).unwrap());
                return r;
            }
        }
    }

    let authed = cookie_token(h).map(|t| token_ok(&app, &t)).unwrap_or(false);
    if !authed {
        return (StatusCode::UNAUTHORIZED, "Open the URL printed by `netusb-client gui` to sign in.").into_response();
    }
    if req.method() != Method::GET {
        let json = h
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.starts_with("application/json"))
            .unwrap_or(false);
        if !json || !origin_ok(&app, h) {
            return (StatusCode::FORBIDDEN, "cross-site or non-JSON write refused").into_response();
        }
    }
    let mut resp = next.run(req).await;
    let hd = resp.headers_mut();
    hd.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    hd.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    hd.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    hd.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; form-action 'none'"),
    );
    resp
}

// ---- API -------------------------------------------------------------------------------------

async fn page() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], UI_HTML)
}

async fn state(State(app): State<App>) -> Json<serde_json::Value> {
    let servers = app.servers.lock().unwrap().clone();
    let mut attachments: Vec<AttachmentView> = app.attachments.lock().unwrap().values().map(|a| a.view.clone()).collect();
    attachments.sort_by(|a, b| a.id.cmp(&b.id));
    let events: Vec<String> = app.events.lock().unwrap().iter().cloned().collect();
    Json(serde_json::json!({ "user": whoami(), "servers": servers, "attachments": attachments, "events": events }))
}

#[derive(Deserialize)]
struct AddrReq {
    addr: String,
}

#[derive(Deserialize)]
struct PairReq {
    addr: String,
    pin: String,
}

#[derive(Deserialize)]
struct UseReq {
    addr: String,
    busid: String,
}

#[derive(Deserialize)]
struct ReleaseReq {
    id: String,
}

fn ok() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

fn clean_addr(raw: &str) -> Result<String, ApiError> {
    let a = with_default_port(raw.trim().to_string());
    if a.len() > 255 || a.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(bad("invalid address"));
    }
    Ok(a)
}

async fn add_server(State(app): State<App>, Json(r): Json<AddrReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let addr = clean_addr(&r.addr)?;
    app.manual.lock().unwrap().insert(addr.clone());
    app.event(format!("added server {addr}"));
    Ok(ok())
}

async fn pair(State(app): State<App>, Json(r): Json<PairReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let addr = clean_addr(&r.addr)?;
    crate::cmd_pair(&app.ctx, &addr, Some(r.pin)).await.map_err(fail)?;
    app.event(format!("paired with {addr}"));
    Ok(ok())
}

async fn use_device(State(app): State<App>, Json(r): Json<UseReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let addr = clean_addr(&r.addr)?;
    let id = format!("{addr}|{}", r.busid);
    if app.attachments.lock().unwrap().contains_key(&id) {
        return Err(bad("already attached"));
    }
    let est = establish(&app.ctx, &addr, &r.busid).await.map_err(fail)?;
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
    tokio::spawn(async move {
        let why = est
            .relay(async move {
                let _ = rx.await;
            })
            .await;
        app2.attachments.lock().unwrap().remove(&id);
        app2.event(format!("{id}: {why}"));
    });
    Ok(ok())
}

async fn release(State(app): State<App>, Json(r): Json<ReleaseReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let stop = app.attachments.lock().unwrap().get_mut(&r.id).and_then(|a| a.stop.take());
    match stop {
        Some(tx) => {
            let _ = tx.send(());
            Ok(ok())
        }
        None => Err(bad("not attached")),
    }
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

fn random_token() -> Result<String> {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("random source failed: {e}"))?;
    Ok(netusb_common::identity::hex(&b))
}

pub async fn run(ctx: Ctx, port: u16) -> Result<()> {
    let token = random_token()?;
    let app: App = Arc::new(Shared {
        ctx,
        token: token.clone(),
        port,
        discovered: Mutex::default(),
        manual: Mutex::default(),
        servers: Mutex::default(),
        attachments: Mutex::default(),
        events: Mutex::default(),
    });

    let router = Router::new()
        .route("/", get(page))
        .route("/api/state", get(state))
        .route("/api/add_server", post(add_server))
        .route("/api/pair", post(pair))
        .route("/api/use", post(use_device))
        .route("/api/release", post(release))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("binding 127.0.0.1:{port} (is another netusb daemon running?)"))?;

    let dir = paths::state_dir()?;
    let url = format!("http://127.0.0.1:{port}/?token={token}");
    write_private(&dir.join("ui.url"), url.as_bytes())?;
    netusb_common::identity::give_to_sudo_user(&dir.join("ui.url"));
    println!("netusb is running. Open this address in your browser:\n\n    {url}\n");
    println!("(also saved in {})", dir.join("ui.url").display());

    spawn_mdns(app.clone());
    tokio::spawn(refresh_loop(app.clone()));

    // On shutdown, unplug everything so no virtual device is left pointing at a dead relay.
    let app_for_stop = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    let stops: Vec<_> = app_for_stop
        .attachments
        .lock()
        .unwrap()
        .values_mut()
        .filter_map(|a| a.stop.take())
        .collect();
    for s in stops {
        let _ = s.send(());
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}
