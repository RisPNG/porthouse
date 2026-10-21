use anyhow::{Context, Result};
use porthouse_client::{service, Ctx};

#[cfg(windows)]
const SERVICE_NAME: &str = "porthouse-client";

fn usage() {
    println!(
        "porthouse-clientd [run] [--importer PATH]   run the client service (needs root, or Administrator on Windows)
porthouse-clientd install-service            Windows: register and start the service
porthouse-clientd uninstall-service          Windows: stop and remove the service"
    );
}

fn importer_from(args: Vec<String>) -> Result<Option<std::path::PathBuf>> {
    let mut importer = std::env::var_os("PORTHOUSE_IMPORTER").map(std::path::PathBuf::from);
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--importer" => importer = Some(args.next().context("--importer needs a path")?.into()),
            other => anyhow::bail!("unknown argument {other:?} (see --help)"),
        }
    }
    Ok(importer)
}

async fn run(importer: Option<std::path::PathBuf>, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
    let ctx = Ctx::load()?;
    eprintln!("porthouse-clientd {} running", env!("CARGO_PKG_VERSION"));
    service::run(ctx, importer, shutdown).await
}

#[cfg(windows)]
fn windows_service_main() -> Result<()> {
    if let Ok(dir) = porthouse_common::paths::state_dir() {
        let _ = porthouse_common::winsvc::log_to(&dir.join("client.log"));
    }
    porthouse_common::winsvc::dispatch(SERVICE_NAME, |stop| {
        let importer = importer_from(Vec::new())?;
        tokio::runtime::Runtime::new()?.block_on(run(importer, async {
            let _ = stop.await;
        }))
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-V") | Some("--version") => {
            println!("porthouse-clientd {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h") | Some("--help") => {
            usage();
            return Ok(());
        }
        #[cfg(windows)]
        Some("service") => return windows_service_main(),
        #[cfg(windows)]
        Some("install-service") => {
            porthouse_common::winsvc::install(
                SERVICE_NAME,
                "Porthouse client",
                "Plugs in USB devices that other computers share.",
                &[],
            )?;
            println!("service {SERVICE_NAME} installed and started");
            return Ok(());
        }
        #[cfg(windows)]
        Some("uninstall-service") => {
            porthouse_common::winsvc::uninstall(SERVICE_NAME)?;
            println!("service {SERVICE_NAME} removed");
            return Ok(());
        }
        Some("run") => {
            args.remove(0);
        }
        _ => {}
    }
    run(importer_from(args)?, porthouse_common::shutdown::console_signal()).await
}
