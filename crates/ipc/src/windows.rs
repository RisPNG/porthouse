use std::ffi::c_void;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

use crate::Service;

/// SYSTEM and administrators have full control, interactive users may read and write.
const SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

fn pipe_name(service: Service) -> String {
    let suffix = std::env::var("NETUSB_IPC_DIR").map(|d| format!("-{d}")).unwrap_or_default();
    let suffix: String = suffix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    format!(r"\\.\pipe\netusb-{}{}", service.name(), suffix)
}

pub(crate) struct Listener {
    name: String,
    next: Option<NamedPipeServer>,
}

fn create(name: &str, first: bool) -> Result<NamedPipeServer> {
    let wide: Vec<u16> = SDDL.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    // SAFETY: `wide` is NUL terminated and outlives the call; `descriptor` receives a buffer that is
    // freed with LocalFree below.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(wide.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut())
    };
    if ok == 0 {
        bail!("building the pipe access list failed: {}", std::io::Error::last_os_error());
    }
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let mut opts = ServerOptions::new();
    opts.first_pipe_instance(first).reject_remote_clients(true);
    // SAFETY: `attrs` points at a valid descriptor for the duration of the call.
    let pipe = unsafe { opts.create_with_security_attributes_raw(name, &mut attrs as *mut _ as *mut c_void) };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW above.
    unsafe { LocalFree(descriptor) };
    pipe.with_context(|| format!("creating {name}"))
}

impl Listener {
    pub(crate) fn bind(service: Service) -> Result<Listener> {
        let name = pipe_name(service);
        let next = create(&name, true)?;
        Ok(Listener { name, next: Some(next) })
    }

    pub(crate) async fn accept(&mut self) -> Result<Option<NamedPipeServer>> {
        let server = match self.next.take() {
            Some(s) => s,
            None => create(&self.name, false)?,
        };
        server.connect().await?;
        self.next = Some(create(&self.name, false)?);
        Ok(Some(server))
    }
}

pub(crate) async fn connect(service: Service) -> Result<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>)> {
    let name = pipe_name(service);
    for _ in 0..20 {
        match ClientOptions::new().open(&name) {
            Ok(c) => {
                let (r, w) = tokio::io::split(c);
                return Ok((Box::new(r), Box::new(w)));
            }
            Err(e) if e.raw_os_error() == Some(231) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("the netusb {} service is not running", service.name()),
            Err(e) => return Err(e).context("opening the control pipe"),
        }
    }
    bail!("the netusb {} service is busy", service.name())
}
