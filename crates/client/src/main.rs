use anyhow::{bail, Context, Result};
use netusb_ipc::{Client, Service};
use serde_json::{json, Value};

fn usage() {
    println!(
        "netusb-client list                          show servers, their devices and what is in use here
netusb-client pair HOST[:PORT] [--pin PIN]  trust a server (PIN from `netusb-server pair`)
netusb-client use HOST[:PORT] BUSID [--seconds N]   attach a device
netusb-client release HOST[:PORT] BUSID     give a device back
netusb-client add HOST[:PORT]               look for a server at this address
netusb-client remove HOST[:PORT]            stop looking at this address
netusb-client servers                       list paired servers
netusb-client forget FINGERPRINT            stop trusting a server
netusb-client ports                         show virtual USB ports (Linux)

These commands talk to the netusb-clientd service, which must be running."
    );
}

fn hex4(v: &Value) -> String {
    format!("{:04x}", v.as_u64().unwrap_or(0))
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn print_state(state: &Value) {
    let servers = state["servers"].as_array().cloned().unwrap_or_default();
    if servers.is_empty() {
        println!("no servers found yet; add one with: netusb-client add HOST");
    }
    for s in &servers {
        let status = if s["error"].is_string() {
            format!("unreachable: {}", text(s, "error"))
        } else if s["paired"] == true {
            "paired".to_string()
        } else {
            "not paired".to_string()
        };
        println!("{} ({})  {status}", text(s, "name"), text(s, "addr"));
        for d in s["devices"].as_array().cloned().unwrap_or_default() {
            let name = [text(&d, "manufacturer"), text(&d, "product")]
                .iter()
                .filter(|p| !p.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let name = if name.is_empty() { "(unnamed device)".to_string() } else { name };
            let mut note = d["in_use_by"].as_str().map(|u| format!("  in use by {u}")).unwrap_or_default();
            if d["input"] == true {
                note.push_str("  [input device: the server loses it while you use it]");
            }
            println!(
                "   {:<8} {}:{}  {name}{note}",
                text(&d, "busid"),
                hex4(&d["vendor_id"]),
                hex4(&d["product_id"])
            );
        }
    }
    for a in state["attachments"].as_array().cloned().unwrap_or_default() {
        println!(
            "in use here: {} from {} (port {})",
            text(&a, "busid"),
            text(&a, "server_name"),
            a["port"]
        );
    }
}

fn with_port(host: &str) -> String {
    if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{}", netusb_proto::DEFAULT_PORT)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref() {
        Some("-V") | Some("--version") => {
            println!("netusb-client {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h") | Some("--help") => {
            usage();
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        Some("ports") => {
            for r in netusb_client::vhci::ports()? {
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
        _ => {}
    }

    let mut c = Client::connect(Service::Client).await?;
    match cmd.as_deref() {
        Some("list") | Some("status") | None => print_state(&c.call("state", Value::Null).await?),
        Some("servers") => {
            let state = c.call("state", Value::Null).await?;
            let known = state["known"].as_array().cloned().unwrap_or_default();
            if known.is_empty() {
                println!("no paired servers");
            }
            for k in known {
                println!("{}  {}", text(&k, "fingerprint"), text(&k, "name"));
            }
        }
        Some("pair") => {
            let host = with_port(&args.next().context("usage: netusb-client pair HOST[:PORT] [--pin PIN]")?);
            let mut pin = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--pin" => pin = Some(args.next().context("--pin needs a value")?),
                    other => bail!("unknown argument {other:?}"),
                }
            }
            let pin = match pin {
                Some(p) => p,
                None => {
                    eprint!("PIN shown by `netusb-server pair` on that machine: ");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    line
                }
            };
            let r = c.call("pair", json!({ "addr": host, "pin": pin })).await?;
            println!("paired with {}", text(&r, "name"));
        }
        Some("use") => {
            let usage = "usage: netusb-client use HOST[:PORT] BUSID [--seconds N]";
            let host = with_port(&args.next().context(usage)?);
            let busid = args.next().context(usage)?;
            let mut seconds = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--seconds" => seconds = Some(args.next().context("--seconds needs a value")?.parse::<u64>()?),
                    other => bail!("unknown argument {other:?}"),
                }
            }
            c.call("use", json!({ "addr": host, "busid": busid, "seconds": seconds })).await?;
            println!("attached {busid}; release it with: netusb-client release {host} {busid}");
        }
        Some("release") => {
            let usage = "usage: netusb-client release HOST[:PORT] BUSID";
            let host = with_port(&args.next().context(usage)?);
            let busid = args.next().context(usage)?;
            c.call("release", json!({ "id": format!("{host}|{busid}") })).await?;
            println!("released {busid}");
        }
        Some("add") => {
            c.call(
                "add_server",
                json!({ "addr": args.next().context("usage: netusb-client add HOST[:PORT]")? }),
            )
            .await?;
        }
        Some("remove") => {
            c.call(
                "remove_server",
                json!({ "addr": args.next().context("usage: netusb-client remove HOST[:PORT]")? }),
            )
            .await?;
        }
        Some("forget") => {
            let fp = args.next().context("usage: netusb-client forget FINGERPRINT")?;
            c.call("forget", json!({ "fingerprint": fp })).await?;
            println!("no longer trusting that server");
        }
        Some(other) => bail!("unknown command {other:?} (see --help)"),
    }
    Ok(())
}
