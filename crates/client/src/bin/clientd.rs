use anyhow::{Context, Result};
use netusb_client::{service, Ctx};

fn usage() {
    println!("netusb-clientd [run] [--importer PATH]   run the client service (needs root, or Administrator on Windows)");
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("run") {
        args.next();
    }
    let mut importer = std::env::var_os("NETUSB_IMPORTER").map(std::path::PathBuf::from);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--importer" => importer = Some(args.next().context("--importer needs a path")?.into()),
            "-V" | "--version" => {
                println!("netusb-clientd {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                usage();
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?} (see --help)"),
        }
    }
    let ctx = Ctx::load()?;
    eprintln!("netusb-clientd {} running", env!("CARGO_PKG_VERSION"));
    service::run(ctx, importer, shutdown_signal()).await
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(t) => t,
            Err(_) => return std::future::pending().await,
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
