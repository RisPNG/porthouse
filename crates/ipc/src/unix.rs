use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};

use crate::Service;

const GROUP: &str = "netusb";

fn own_uid() -> u32 {
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX)
}

fn run_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NETUSB_IPC_DIR") {
        return PathBuf::from(d);
    }
    if own_uid() == 0 {
        return PathBuf::from("/run/netusb");
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(x) => PathBuf::from(x).join("netusb"),
        None => PathBuf::from(format!("/tmp/netusb-{}", own_uid())),
    }
}

fn socket_path(service: Service) -> PathBuf {
    run_dir().join(format!("{}.sock", service.name()))
}

/// Every place a service might have put its socket, best guess first.
fn candidates(service: Service) -> Vec<PathBuf> {
    let file = format!("{}.sock", service.name());
    if let Some(d) = std::env::var_os("NETUSB_IPC_DIR") {
        return vec![PathBuf::from(d).join(file)];
    }
    let mut v = vec![PathBuf::from("/run/netusb").join(&file)];
    if let Some(x) = std::env::var_os("XDG_RUNTIME_DIR") {
        v.push(PathBuf::from(x).join("netusb").join(&file));
    }
    v
}

pub(crate) struct Listener {
    inner: UnixListener,
    service_uid: u32,
}

impl Listener {
    pub(crate) fn bind(service: Service) -> Result<Listener> {
        let path = socket_path(service);
        let dir = path.parent().context("socket path has no directory")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let mode = if own_uid() == 0 { 0o755 } else { 0o700 };
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))?;
        let _ = std::fs::remove_file(&path);
        let inner = UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        // Access is decided per connection from the peer's credentials, not from file modes.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))?;
        Ok(Listener { inner, service_uid: own_uid() })
    }

    /// `Ok(None)` means a peer connected but is not allowed to use the service.
    pub(crate) async fn accept(&mut self) -> Result<Option<UnixStream>> {
        let (stream, _) = self.inner.accept().await?;
        let cred = stream.peer_cred()?;
        if allowed(cred.uid(), cred.gid(), self.service_uid) {
            Ok(Some(stream))
        } else {
            Ok(None)
        }
    }
}

/// Root, the account running the service, members of group `netusb`, and anyone logged in at a
/// local seat may use the service. The last rule is what lets the desktop user run the program
/// without any set-up.
fn allowed(uid: u32, gid: u32, service_uid: u32) -> bool {
    if uid == 0 || uid == service_uid {
        return true;
    }
    if service_uid != 0 {
        return false;
    }
    in_group(uid, gid) || has_session(uid)
}

fn in_group(uid: u32, primary_gid: u32) -> bool {
    let Ok(groups) = std::fs::read_to_string("/etc/group") else { return false };
    let Some(line) = groups.lines().find(|l| l.split(':').next() == Some(GROUP)) else { return false };
    let f: Vec<&str> = line.split(':').collect();
    if f.get(2).and_then(|g| g.parse::<u32>().ok()) == Some(primary_gid) {
        return true;
    }
    let Some(name) = user_name(uid) else { return false };
    f.get(3).map(|m| m.split(',').any(|u| u == name)).unwrap_or(false)
}

fn user_name(uid: u32) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.get(2).and_then(|u| u.parse::<u32>().ok()) == Some(uid)).then(|| f[0].to_string())
    })
}

/// systemd-logind keeps a state file for every user with a login session.
fn has_session(uid: u32) -> bool {
    std::fs::read_to_string(format!("/run/systemd/users/{uid}"))
        .map(|s| s.lines().any(|l| l == "STATE=active" || l == "STATE=online"))
        .unwrap_or(false)
}

pub(crate) async fn connect(
    service: Service,
) -> Result<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>)> {
    let mut last = None;
    for path in candidates(service) {
        match UnixStream::connect(&path).await {
            Ok(s) => {
                let (r, w) = s.into_split();
                return Ok((Box::new(r), Box::new(w)));
            }
            Err(e) => last = Some((path, e)),
        }
    }
    match last {
        Some((path, e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!("not allowed to use the netusb {} service ({}); add yourself to the `netusb` group", service.name(), path.display())
        }
        _ => bail!("the netusb {} service is not running", service.name()),
    }
}
