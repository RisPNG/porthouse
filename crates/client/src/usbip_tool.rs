use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use porthouse_proto::AttachInfo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const USBIP_VERSION: u16 = 0x0111;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;
const BUS_ID_SIZE: usize = 32;
const DEV_PATH_MAX: usize = 256;
pub const USB_DEVICE_LEN: usize = DEV_PATH_MAX + BUS_ID_SIZE + 12 + 6 + 6;

const ST_OK: u32 = 0;
const ST_NA: u32 = 1;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const ATTACH_TIMEOUT: Duration = Duration::from_secs(30);

fn put_str(buf: &mut Vec<u8>, s: &str, len: usize) {
    let b = s.as_bytes();
    let n = b.len().min(len - 1); // always leave a NUL terminator
    buf.extend_from_slice(&b[..n]);
    buf.resize(buf.len() + (len - n), 0);
}

fn op_header(code: u16, status: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(8);
    h.extend_from_slice(&USBIP_VERSION.to_be_bytes());
    h.extend_from_slice(&code.to_be_bytes());
    h.extend_from_slice(&status.to_be_bytes());
    h
}

/// A successful `OP_REP_IMPORT`: header plus the device description.
///
/// `busid` must be exactly the bus id the importer asked for: importers (the reference Linux tool
/// and the usbip-win2 driver alike) reject a reply that names a different one as a protocol
/// violation. The real bus id is only informational and travels in `path`.
pub fn import_reply(info: &AttachInfo, path: &str, busid: &str) -> Vec<u8> {
    let mut b = op_header(OP_REP_IMPORT, ST_OK);
    put_str(&mut b, path, DEV_PATH_MAX);
    put_str(&mut b, busid, BUS_ID_SIZE);
    b.extend_from_slice(&info.busnum.to_be_bytes());
    b.extend_from_slice(&info.devnum.to_be_bytes());
    b.extend_from_slice(&info.speed.to_be_bytes());
    b.extend_from_slice(&info.vendor_id.to_be_bytes());
    b.extend_from_slice(&info.product_id.to_be_bytes());
    b.extend_from_slice(&info.bcd_device.to_be_bytes());
    b.extend_from_slice(&[
        info.class,
        info.subclass,
        info.protocol,
        info.configuration_value,
        info.num_configurations,
        info.num_interfaces,
    ]);
    debug_assert_eq!(b.len(), 8 + USB_DEVICE_LEN);
    b
}

pub fn import_refusal() -> Vec<u8> {
    op_header(OP_REP_IMPORT, ST_NA)
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Import { busid: String },
    Other,
}

async fn read_request(s: &mut TcpStream) -> Result<Request> {
    let mut hdr = [0u8; 8];
    s.read_exact(&mut hdr).await?;
    let version = u16::from_be_bytes([hdr[0], hdr[1]]);
    let code = u16::from_be_bytes([hdr[2], hdr[3]]);
    if version != USBIP_VERSION || code != OP_REQ_IMPORT {
        return Ok(Request::Other);
    }
    let mut id = [0u8; BUS_ID_SIZE];
    s.read_exact(&mut id).await?;
    let end = id.iter().position(|b| *b == 0).unwrap_or(BUS_ID_SIZE);
    Ok(Request::Import {
        busid: String::from_utf8_lossy(&id[..end]).into_owned(),
    })
}

/// Wait for the importer's connection, require it to present `token` as the bus id, answer the
/// import with `info`, and return the connected socket now carrying raw URB traffic.
///
/// Anything else that connects (wrong request, wrong token, junk, silence) is refused or dropped
/// and the wait continues, so a stray local process cannot take the device. The token is random
/// per attach and is the only thing that lets a connection through.
pub async fn accept_import(listener: &TcpListener, token: &str, info: &AttachInfo, path: &str) -> Result<TcpStream> {
    accept_import_with(listener, token, info, path, HANDSHAKE_TIMEOUT).await
}

async fn accept_import_with(listener: &TcpListener, token: &str, info: &AttachInfo, path: &str, per_conn: Duration) -> Result<TcpStream> {
    loop {
        let (mut s, peer) = listener.accept().await.context("accepting the importer")?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let _ = s.set_nodelay(true);
        match tokio::time::timeout(per_conn, read_request(&mut s)).await {
            Ok(Ok(Request::Import { busid })) if busid == token => {
                s.write_all(&import_reply(info, path, token))
                    .await
                    .context("sending the import reply")?;
                return Ok(s);
            }
            Ok(Ok(_)) => {
                let _ = s.write_all(&import_refusal()).await;
            }
            Ok(Err(_)) | Err(_) => {}
        }
    }
}

pub fn find_tool() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("PORTHOUSE_USBIP_EXE") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            bail!("PORTHOUSE_USBIP_EXE points at {}, which is not a file", p.display())
        };
    }
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramW6432"] {
        if let Some(dir) = std::env::var_os(var) {
            let p = PathBuf::from(dir).join("USBip").join("usbip.exe");
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    let name = if cfg!(windows) { "usbip.exe" } else { "usbip" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    bail!(
        "could not find the USB/IP importer ({name}). Install usbip-win2 from https://github.com/vadimgrn/usbip-win2/releases, \
         or set PORTHOUSE_USBIP_EXE to its full path"
    )
}

fn command(exe: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(exe);
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing from a GUI-driven daemon
    c
}

fn random_token() -> Result<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("random source failed: {e}"))?;
    Ok(format!("nu{}", porthouse_common::identity::hex(&b)))
}

pub async fn attach(exe: &Path, info: &AttachInfo) -> Result<(TcpStream, u32)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = random_token()?;

    let child = command(exe)
        .args([
            "--tcp-port",
            &port.to_string(),
            "attach",
            "-r",
            "127.0.0.1",
            "-b",
            &token,
            "--once",
            "--terse",
        ])
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;

    let path = format!("porthouse/{}", info.busid);
    let serve = accept_import(&listener, &token, info, &path);
    let wait = child.wait_with_output();
    tokio::pin!(serve, wait);

    let mut local: Option<TcpStream> = None;
    let output = tokio::time::timeout(ATTACH_TIMEOUT, async {
        loop {
            tokio::select! {
                r = &mut serve, if local.is_none() => local = Some(r?),
                out = &mut wait => return out.context("waiting for the importer"),
            }
        }
    })
    .await
    .context("timed out waiting for the USB/IP importer to attach the device")??;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        bail!("the USB/IP importer failed ({}): {}{}", output.status, err.trim(), out.trim());
    }
    let local = match local {
        Some(l) => l,
        // The importer can exit just before our handshake future is polled to completion.
        None => tokio::time::timeout(Duration::from_secs(5), &mut serve)
            .await
            .context("the importer finished but never completed the handshake")??,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let port: u32 = stdout
        .split_whitespace()
        .next()
        .and_then(|t| t.parse().ok())
        .with_context(|| format!("could not read the port number from the importer's output: {:?}", stdout.trim()))?;
    Ok((local, port))
}

pub async fn detach(exe: &Path, port: u32) -> Result<()> {
    let out = tokio::time::timeout(
        Duration::from_secs(15),
        command(exe).args(["detach", "-p", &port.to_string()]).output(),
    )
    .await
    .context("timed out detaching")?
    .with_context(|| format!("starting {}", exe.display()))?;
    if !out.status.success() {
        bail!("detach failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[allow(dead_code)]
fn is_local(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> AttachInfo {
        AttachInfo {
            busid: "5-1".into(),
            busnum: 5,
            devnum: 2,
            speed: 2,
            vendor_id: 0x1532,
            product_id: 0x0098,
            bcd_device: 0x0200,
            class: 0,
            subclass: 0,
            protocol: 0,
            configuration_value: 1,
            num_configurations: 1,
            num_interfaces: 3,
        }
    }

    fn import_request(busid: &str) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&USBIP_VERSION.to_be_bytes());
        r.extend_from_slice(&OP_REQ_IMPORT.to_be_bytes());
        r.extend_from_slice(&0u32.to_be_bytes());
        put_str(&mut r, busid, BUS_ID_SIZE);
        r
    }

    async fn listener() -> (TcpListener, u16) {
        let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let p = l.local_addr().unwrap().port();
        (l, p)
    }

    #[test]
    fn reply_matches_the_usbip_wire_layout() {
        let r = import_reply(&info(), "porthouse/5-1", "nuTOKEN");
        assert_eq!(r.len(), 8 + 312, "op_common + usbip_usb_device");
        assert_eq!(&r[0..2], &[0x01, 0x11], "protocol version 0x0111");
        assert_eq!(&r[2..4], &[0x00, 0x03], "OP_REP_IMPORT");
        assert_eq!(&r[4..8], &[0, 0, 0, 0], "status OK");
        assert_eq!(&r[8..18], b"porthouse/5-1");
        assert_eq!(r[18], 0, "path is NUL padded");
        let busid = 8 + 256;
        assert_eq!(
            &r[busid..busid + 7],
            b"nuTOKEN",
            "the reply echoes the bus id the importer asked for, not the real one"
        );
        let n = busid + 32;
        assert_eq!(&r[n..n + 4], &5u32.to_be_bytes(), "busnum");
        assert_eq!(&r[n + 4..n + 8], &2u32.to_be_bytes(), "devnum");
        assert_eq!(&r[n + 8..n + 12], &2u32.to_be_bytes(), "speed: full");
        assert_eq!(&r[n + 12..n + 14], &0x1532u16.to_be_bytes(), "idVendor");
        assert_eq!(&r[n + 14..n + 16], &0x0098u16.to_be_bytes(), "idProduct");
        assert_eq!(&r[n + 16..n + 18], &0x0200u16.to_be_bytes(), "bcdDevice");
        assert_eq!(
            &r[n + 18..n + 24],
            &[0, 0, 0, 1, 1, 3],
            "class/sub/proto/config/nconfigs/ninterfaces"
        );
    }

    #[test]
    fn long_strings_are_truncated_with_a_terminator() {
        let mut b = Vec::new();
        put_str(&mut b, &"x".repeat(100), 32);
        assert_eq!(b.len(), 32);
        assert_eq!(b[31], 0, "last byte stays NUL");
    }

    #[test]
    fn refusal_is_a_bare_header_with_nonzero_status() {
        let r = import_refusal();
        assert_eq!(r.len(), 8);
        assert_eq!(&r[2..4], &[0x00, 0x03]);
        assert_ne!(&r[4..8], &[0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn the_right_token_gets_the_device_and_a_live_socket() {
        let (l, port) = listener().await;
        let i = info();
        let server = tokio::spawn(async move { accept_import(&l, "nuSECRET", &i, "p").await });
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(&import_request("nuSECRET")).await.unwrap();
        let mut reply = vec![0u8; 8 + USB_DEVICE_LEN];
        c.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply[2..4], &[0x00, 0x03]);
        assert_eq!(&reply[4..8], &[0, 0, 0, 0]);
        // After the handshake the stream is a plain byte pipe in both directions.
        let mut s = server.await.unwrap().unwrap();
        c.write_all(b"URB-DATA-FROM-DRIVER").await.unwrap();
        let mut got = [0u8; 20];
        s.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"URB-DATA-FROM-DRIVER");
        s.write_all(b"REPLY").await.unwrap();
        let mut back = [0u8; 5];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"REPLY");
    }

    #[tokio::test]
    async fn a_wrong_token_is_refused_and_does_not_end_the_wait() {
        let (l, port) = listener().await;
        let i = info();
        let server = tokio::spawn(async move { accept_import(&l, "nuSECRET", &i, "p").await });

        let mut intruder = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        intruder.write_all(&import_request("nuGUESS")).await.unwrap();
        let mut hdr = [0u8; 8];
        intruder.read_exact(&mut hdr).await.unwrap();
        assert_ne!(&hdr[4..8], &[0, 0, 0, 0], "intruder gets a refusal, not the device");
        assert_eq!(intruder.read(&mut [0u8; 1]).await.unwrap(), 0, "and is disconnected");

        // The legitimate importer still gets through afterwards.
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(&import_request("nuSECRET")).await.unwrap();
        let mut reply = vec![0u8; 8 + USB_DEVICE_LEN];
        c.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply[4..8], &[0, 0, 0, 0]);
        assert!(server.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn wrong_version_or_operation_is_refused() {
        let (l, port) = listener().await;
        let i = info();
        let server = tokio::spawn(async move { accept_import(&l, "t", &i, "p").await });
        // A device-list request (0x8005) is not an import.
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(&[0x01, 0x11, 0x80, 0x05, 0, 0, 0, 0]).await.unwrap();
        let mut hdr = [0u8; 8];
        c.read_exact(&mut hdr).await.unwrap();
        assert_ne!(&hdr[4..8], &[0, 0, 0, 0]);
        // Wrong protocol version.
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(&[0x01, 0x00, 0x80, 0x03, 0, 0, 0, 0]).await.unwrap();
        c.read_exact(&mut hdr).await.unwrap();
        assert_ne!(&hdr[4..8], &[0, 0, 0, 0]);
        server.abort();
    }

    #[tokio::test]
    async fn a_silent_or_junk_connection_cannot_block_the_real_importer() {
        let (l, port) = listener().await;
        let i = info();
        let server = tokio::spawn(async move { accept_import_with(&l, "nuSECRET", &i, "p", Duration::from_millis(200)).await });
        let _silent = TcpStream::connect(("127.0.0.1", port)).await.unwrap(); // never sends anything
        let mut junk = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        junk.write_all(b"\x00\x01").await.unwrap(); // starts a header, never finishes it
        let mut c = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        c.write_all(&import_request("nuSECRET")).await.unwrap();
        let mut reply = vec![0u8; 8 + USB_DEVICE_LEN];
        tokio::time::timeout(Duration::from_secs(5), c.read_exact(&mut reply))
            .await
            .expect("handshake stalled")
            .unwrap();
        assert_eq!(&reply[4..8], &[0, 0, 0, 0]);
        assert!(server.await.unwrap().is_ok());
    }

    // ---- orchestration against a fake importer (bash, so Unix only) -------------------------

    #[cfg(unix)]
    mod fake_importer {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn script(dir: &Path, body: &str) -> PathBuf {
            let p = dir.join("usbip");
            std::fs::write(&p, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        fn tmp(tag: &str) -> PathBuf {
            let d = std::env::temp_dir().join(format!("porthouse-tool-{tag}-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        /// Behaves like `usbip.exe --tcp-port P attach -r H -b ID --once --terse`: connects, imports,
        /// prints the port, and leaves the connection open in a background process (the kernel
        /// driver, in the real thing, is what keeps the socket).
        const WELL_BEHAVED: &str = r#"
set -e
[ "$1" = "--tcp-port" ] && [ "$3" = "attach" ] && [ "$4" = "-r" ] && [ "$5" = "127.0.0.1" ] && [ "$6" = "-b" ] && [ "$8" = "--once" ] && [ "$9" = "--terse" ] || { echo "unexpected args: $*" >&2; exit 2; }
port=$2; id=$7
exec 3<>/dev/tcp/127.0.0.1/$port
printf '\x01\x11\x80\x03\x00\x00\x00\x00' >&3
printf '%-32s' "$id" | tr ' ' '\000' >&3
printf '%s' "$id" > "$(dirname "$0")/id.txt"
head -c 320 <&3 > "$(dirname "$0")/reply.bin"
( sleep 30 <&3 >/dev/null 2>&1 & )
echo 7
"#;

        #[tokio::test]
        async fn attach_runs_the_importer_serves_the_handshake_and_returns_the_port() {
            let d = tmp("ok");
            let exe = script(&d, WELL_BEHAVED);
            let (mut local, port) = attach(&exe, &info()).await.unwrap();
            assert_eq!(port, 7, "port parsed from --terse output");
            let reply = std::fs::read(d.join("reply.bin")).unwrap();
            let id = std::fs::read_to_string(d.join("id.txt")).unwrap();
            assert!(
                id.starts_with("nu") && id.len() == 18,
                "the importer was given a random token as its bus id: {id:?}"
            );
            assert_eq!(
                reply,
                import_reply(&info(), "porthouse/5-1", &id),
                "importer received exactly our device description"
            );
            // The accepted socket is live: the driver side (a background process) still holds it.
            local.write_all(b"x").await.unwrap();
            let _ = std::fs::remove_dir_all(&d);
        }

        #[tokio::test]
        async fn a_failing_importer_surfaces_its_error() {
            let d = tmp("fail");
            let exe = script(&d, "echo 'driver is not installed' >&2; exit 3");
            let err = attach(&exe, &info()).await.expect_err("must fail").to_string();
            assert!(err.contains("driver is not installed"), "{err}");
            let _ = std::fs::remove_dir_all(&d);
        }

        #[tokio::test]
        async fn an_importer_that_never_imports_does_not_hang_forever() {
            let d = tmp("hang");
            // Exits successfully without ever connecting: attach must not report success.
            let exe = script(&d, "echo 4");
            let r = tokio::time::timeout(Duration::from_secs(20), attach(&exe, &info()))
                .await
                .expect("attach hung");
            assert!(r.is_err(), "no handshake happened, so this cannot be a success");
            let _ = std::fs::remove_dir_all(&d);
        }

        #[tokio::test]
        async fn an_unparseable_port_is_an_error() {
            let d = tmp("port");
            let exe = script(&d, &WELL_BEHAVED.replace("echo 7", "echo 'attached somewhere'"));
            let err = attach(&exe, &info()).await.expect_err("must fail").to_string();
            assert!(err.contains("port number"), "{err}");
            let _ = std::fs::remove_dir_all(&d);
        }

        #[tokio::test]
        async fn detach_passes_the_port() {
            let d = tmp("detach");
            let exe = script(&d, r#"echo "$@" > "$(dirname "$0")/args.txt""#);
            detach(&exe, 12).await.unwrap();
            assert_eq!(std::fs::read_to_string(d.join("args.txt")).unwrap().trim(), "detach -p 12");
            let bad = script(&d, "echo nope >&2; exit 1");
            assert!(detach(&bad, 1).await.is_err());
            let _ = std::fs::remove_dir_all(&d);
        }
    }
}
