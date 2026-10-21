use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use netusb_client::*;

fn usage() {
    println!(
        "netusb-client list [--host HOST[:PORT]]... [--wait SECONDS]   find servers and show their devices
netusb-client pair HOST[:PORT] [--pin PIN]                     trust a server (PIN from `netusb-server pair`)
netusb-client use HOST[:PORT] BUSID [--importer PATH]          attach a device (Linux: needs root; Windows: needs usbip-win2)
netusb-client servers                                          list paired servers
netusb-client forget FINGERPRINT                               stop trusting a server
netusb-client ports                                            show virtual USB ports (Linux)
netusb-client open                                             open the running web interface in your browser
netusb-client gui [--port N]                                   start the web interface (needs root to attach devices)"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    let ctx = Ctx::load()?;
    match cmd.as_deref() {
        Some("pair") => {
            let host = with_default_port(args.next().context("usage: netusb-client pair HOST[:PORT] [--pin PIN]")?);
            let mut pin = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--pin" => pin = Some(args.next().context("--pin needs a value")?),
                    other => bail!("unknown argument {other:?}"),
                }
            }
            return cmd_pair(&ctx, &host, pin).await;
        }
        Some("use") => {
            let host = args.next().context("usage: netusb-client use HOST[:PORT] BUSID")?;
            let busid = args.next().context("usage: netusb-client use HOST[:PORT] BUSID")?;
            let mut importer = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--importer" => importer = Some(std::path::PathBuf::from(args.next().context("--importer needs a path")?)),
                    other => bail!("unknown argument {other:?}"),
                }
            }
            return cmd_use(&ctx, &with_default_port(host), &busid, importer).await;
        }
        Some("servers") => {
            let all = ctx.known.entries();
            if all.is_empty() {
                println!("no paired servers");
            }
            for (fp, name) in all {
                println!("{fp}  {name}");
            }
            return Ok(());
        }
        Some("forget") => {
            let want = args.next().context("usage: netusb-client forget FINGERPRINT")?;
            let m: Vec<_> = ctx.known.entries().into_iter().filter(|(f, _)| f.starts_with(&want)).collect();
            return match m.as_slice() {
                [(f, n)] => {
                    ctx.known.remove(f)?;
                    println!("no longer trusting {n} ({f})");
                    Ok(())
                }
                [] => bail!("no paired server matches {want:?}"),
                _ => bail!("{want:?} matches several servers; use more characters"),
            };
        }
        #[cfg(target_os = "linux")]
        Some("ports") => {
            for r in vhci::ports()? {
                println!(
                    "{:<2} port {:>2}  state {}  {}",
                    if r.hub_super { "ss" } else { "hs" },
                    r.port,
                    r.state,
                    r.busid
                );
            }
            return Ok(());
        }
        Some("open") => return cmd_open(),
        Some("daemon") | Some("gui") => {
            let mut port = 7601u16;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--port" => port = args.next().context("--port needs a value")?.parse()?,
                    other => bail!("unknown argument {other:?}"),
                }
            }
            return netusb_client::daemon::run(ctx, port).await;
        }
        Some("-h") | Some("--help") => {
            usage();
            return Ok(());
        }
        Some("list") | None => {}
        Some(other) => bail!("unknown command {other:?} (see --help)"),
    }

    let mut hosts: Vec<String> = Vec::new();
    let mut wait = Duration::from_secs(3);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => hosts.push(with_default_port(args.next().context("--host needs a value")?)),
            "--wait" => wait = Duration::from_secs(args.next().context("--wait needs a value")?.parse()?),
            other => bail!("unknown argument {other:?}"),
        }
    }

    if hosts.is_empty() {
        eprintln!("searching the network for {}s...", wait.as_secs());
        hosts = tokio::task::spawn_blocking(move || discover(wait)).await??;
        if hosts.is_empty() {
            bail!("no servers found; pass --host HOST[:PORT] to connect directly");
        }
    }

    let ctx = Arc::new(ctx);
    let mut failures = 0;
    // A host reachable on several addresses is announced once per address; show it once.
    let mut seen_ids: Vec<String> = Vec::new();
    for h in &hosts {
        match query(&ctx, h).await {
            Ok(l) if seen_ids.contains(&l.server_id) => {}
            Ok(l) => {
                seen_ids.push(l.server_id.clone());
                print_listing(&l);
            }
            Err(e) => {
                failures += 1;
                eprintln!("{h}: {e:#}");
            }
        }
    }
    if failures == hosts.len() {
        bail!("no server could be queried");
    }
    Ok(())
}
