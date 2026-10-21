use std::ffi::OsString;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::sync::oneshot;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub type Entry = fn(oneshot::Receiver<()>) -> Result<()>;

static ENTRY: OnceLock<(&'static str, Entry)> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    let Some((name, entry)) = ENTRY.get() else { return };
    if let Err(e) = run(name, *entry) {
        eprintln!("service {name} failed: {e:#}");
    }
}

pub fn dispatch(name: &'static str, entry: Entry) -> Result<()> {
    ENTRY
        .set((name, entry))
        .map_err(|_| anyhow::anyhow!("the service was already started"))?;
    service_dispatcher::start(name, ffi_service_main).context("this program must be started by the service manager")?;
    Ok(())
}

fn status(state: ServiceState, exit: u32) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(exit),
        checkpoint: 0,
        wait_hint: Duration::from_secs(20),
        process_id: None,
    }
}

fn run(name: &'static str, entry: Entry) -> Result<()> {
    let (stop_tx, stop_rx) = oneshot::channel();
    let stop_tx = Mutex::new(Some(stop_tx));
    let handle = service_control_handler::register(name, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(tx) = stop_tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    handle.set_service_status(status(ServiceState::Running, 0))?;
    let result = entry(stop_rx);
    handle.set_service_status(status(ServiceState::Stopped, u32::from(result.is_err())))?;
    result
}

pub fn log_to(path: &std::path::Path) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    // SAFETY: the handle stays open for the rest of the process because the file is leaked.
    let ok = unsafe { set_std_handle(-12i32 as u32, file.as_raw_handle()) } != 0;
    std::mem::forget(file);
    if !ok {
        bail!("cannot redirect the log: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

extern "system" {
    #[link_name = "SetStdHandle"]
    fn set_std_handle(which: u32, handle: std::os::windows::io::RawHandle) -> i32;
}

pub fn install(name: &str, display: &str, description: &str, dependencies: &[&str]) -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .context("this needs Administrator rights")?;
    let info = ServiceInfo {
        name: OsString::from(name),
        display_name: OsString::from(display),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe()?,
        launch_arguments: vec![OsString::from("service")],
        dependencies: dependencies
            .iter()
            .map(|d| windows_service::service::ServiceDependency::Service(OsString::from(*d)))
            .collect(),
        account_name: None,
        account_password: None,
    };
    let access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    let service = match manager.create_service(&info, access) {
        Ok(s) => s,
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1073) => {
            // ERROR_SERVICE_EXISTS: update it so a reinstall points at the new program.
            let s = manager.open_service(name, access)?;
            s.change_config(&info)?;
            s
        }
        Err(e) => return Err(e).context("creating the service"),
    };
    service.set_description(description)?;
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: Duration::from_secs(5),
        }]),
    })?;
    if service.query_status()?.current_state != ServiceState::Running {
        service.start::<&str>(&[])?;
    }
    Ok(())
}

pub fn uninstall(name: &str) -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).context("this needs Administrator rights")?;
    let access = ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS;
    let service = match manager.open_service(name, access) {
        Ok(s) => s,
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => return Ok(()), // ERROR_SERVICE_DOES_NOT_EXIST
        Err(e) => return Err(e).context("opening the service"),
    };
    if service.query_status()?.current_state != ServiceState::Stopped {
        let _ = service.stop();
        for _ in 0..60 {
            if service.query_status()?.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    service.delete()?;
    Ok(())
}
