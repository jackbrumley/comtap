//! The Windows service: registration, start/stop, and the entry point Windows calls.
use crate::log;
use anyhow::{Context, Result};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl,
    ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState,
    ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const NAME: &str = "ComTap";
const DISPLAY: &str = "ComTap (TCP device to virtual COM port)";
const DESCRIPTION: &str = "Bridges TCP devices, such as Ethernet barcode scanners, to com0com virtual COM ports.";

define_windows_service!(ffi_service_main, service_main);

/// Called when Windows starts `comtap.exe service`.
pub fn dispatch() -> Result<()> {
    service_dispatcher::start(NAME, ffi_service_main).context("not started by the Service Control Manager")
}

fn service_main(_args: Vec<OsString>) {
    crate::logger::init(crate::paths::logs_dir(), false);
    if let Err(e) = run_service() {
        log!("Service error: {e:#}");
    }
}

fn run_service() -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = stop.clone();
    let handle = service_control_handler::register(NAME, move |ctrl| match ctrl {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            handler_stop.store(true, Ordering::Relaxed);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let status = |state, accept, code| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    };
    handle.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    ))?;
    log!("Service started (ComTap {})", env!("CARGO_PKG_VERSION"));
    let result = crate::run_all(&crate::paths::installed_config(), stop);
    let code = match &result {
        Ok(()) => 0,
        Err(e) => {
            log!("Service stopping on error: {e:#}");
            1
        }
    };
    log!("Service stopped");
    // A non-zero exit code makes Windows apply the restart-on-failure actions.
    handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty(), code))?;
    Ok(())
}

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access).context("opening the Service Control Manager")
}

/// True if this process can create services (i.e. is running as administrator).
pub fn is_admin() -> bool {
    ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE).is_ok()
}

/// Creates the service, or updates it if it is already there. Auto-start, restart on failure.
pub fn install(exe: &Path) -> Result<()> {
    let m = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: DISPLAY.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.to_path_buf(),
        launch_arguments: vec!["service".into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS;
    let svc = match m.open_service(NAME, access) {
        Ok(s) => {
            s.change_config(&info)?;
            s
        }
        Err(_) => m.create_service(&info, access)?,
    };
    svc.set_description(DESCRIPTION)?;
    let restart = ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(5) };
    svc.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![restart.clone(), restart.clone(), restart]),
    })?;
    svc.set_failure_actions_on_non_crash_failures(true)?;
    Ok(())
}

fn wait_for(svc: &windows_service::service::Service, want: ServiceState, secs: u64) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let s = svc.query_status()?.current_state;
        if s == want {
            return Ok(());
        }
        if Instant::now() > end {
            anyhow::bail!("service is {s:?}, expected {want:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn start() -> Result<()> {
    let m = manager(ServiceManagerAccess::CONNECT)?;
    let svc = m.open_service(NAME, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
    if svc.query_status()?.current_state != ServiceState::Running {
        svc.start::<&OsStr>(&[])?;
        wait_for(&svc, ServiceState::Running, 20)?;
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    let m = manager(ServiceManagerAccess::CONNECT)?;
    let Ok(svc) = m.open_service(NAME, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS) else {
        return Ok(());
    };
    if svc.query_status()?.current_state != ServiceState::Stopped {
        let _ = svc.stop();
        wait_for(&svc, ServiceState::Stopped, 20)?;
    }
    Ok(())
}

pub fn delete() -> Result<()> {
    stop()?;
    let m = manager(ServiceManagerAccess::CONNECT)?;
    if let Ok(svc) = m.open_service(NAME, ServiceAccess::DELETE) {
        svc.delete()?;
    }
    Ok(())
}

/// "Running", "Stopped", ... or "not installed".
pub fn state_text() -> String {
    let Ok(m) = manager(ServiceManagerAccess::CONNECT) else { return "unknown".into() };
    match m.open_service(NAME, ServiceAccess::QUERY_STATUS) {
        Ok(s) => match s.query_status() {
            Ok(st) => format!("{:?}", st.current_state),
            Err(e) => format!("unknown ({e})"),
        },
        Err(_) => "not installed".into(),
    }
}
