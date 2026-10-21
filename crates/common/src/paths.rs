use std::path::PathBuf;

pub fn state_dir() -> std::io::Result<PathBuf> {
    let dir = match std::env::var_os("NETUSB_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => default_dir()?,
    };
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

#[cfg(unix)]
fn default_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let root = std::fs::metadata("/proc/self").map(|m| m.uid() == 0).unwrap_or(false);
    if root {
        return Ok(PathBuf::from("/var/lib/netusb"));
    }
    if let Some(x) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(x).join("netusb"));
    }
    match std::env::var_os("HOME") {
        Some(h) => Ok(PathBuf::from(h).join(".local/state/netusb")),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot determine a state directory; set NETUSB_STATE_DIR",
        )),
    }
}

#[cfg(windows)]
fn default_dir() -> std::io::Result<PathBuf> {
    match std::env::var_os("ProgramData") {
        Some(d) => Ok(PathBuf::from(d).join("netusb")),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cannot determine a state directory; set NETUSB_STATE_DIR",
        )),
    }
}
