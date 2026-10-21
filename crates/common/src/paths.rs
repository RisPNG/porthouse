//! Where netusb keeps its identity and trust files.
//!
//! Under `sudo`, the invoking user's directory is used (so `sudo netusb-client use ...` sees the
//! pairing that user made). A system service has no `SUDO_USER` and uses `/var/lib/netusb`.

use std::path::PathBuf;

/// State directory for the server.
///
/// On Windows the server runs as SYSTEM but is administered by a person in another profile, so
/// both must agree on one location: `%ProgramData%\netusb` (locked to administrators by the
/// installer). Elsewhere it is the same as [`state_dir`].
pub fn server_state_dir() -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    if std::env::var_os("NETUSB_STATE_DIR").is_none() {
        if let Some(pd) = std::env::var_os("ProgramData") {
            let dir = PathBuf::from(pd).join("netusb");
            std::fs::create_dir_all(&dir)?;
            return Ok(dir);
        }
    }
    state_dir()
}

/// State directory, created on demand with owner-only permissions.
///
/// Order: `NETUSB_STATE_DIR`; `/var/lib/netusb` for root on Unix; `$XDG_STATE_HOME/netusb` or
/// `~/.local/state/netusb`; `%APPDATA%\netusb` on Windows.
pub fn state_dir() -> std::io::Result<PathBuf> {
    let dir = if let Some(d) = std::env::var_os("NETUSB_STATE_DIR") {
        PathBuf::from(d)
    } else if let Some(d) = platform_dir() {
        d
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot determine a state directory; set NETUSB_STATE_DIR",
        ));
    };
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        crate::identity::give_to_sudo_user(&dir);
    }
    Ok(dir)
}

#[cfg(unix)]
fn platform_dir() -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1);
    if uid == 0 {
        if let Some(home) = std::env::var("SUDO_USER").ok().and_then(|u| home_of(&u)) {
            return Some(home.join(".local/state/netusb"));
        }
        return Some(PathBuf::from("/var/lib/netusb"));
    }
    if let Some(x) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(x).join("netusb"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state/netusb"))
}

/// Home directory of `user` from /etc/passwd (name:pw:uid:gid:gecos:home:shell).
#[cfg(unix)]
fn home_of(user: &str) -> Option<PathBuf> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 6 && f[0] == user && !f[5].is_empty()).then(|| PathBuf::from(f[5]))
    })
}

#[cfg(not(unix))]
fn platform_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("netusb"))
}
