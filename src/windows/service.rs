/*
 注册成windows服务，开机运行
 tasklist | findstr supervisord
*/
#![allow(dead_code)]
use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

//sc.exe start supervisord
fn service_main(arguments: Vec<OsString>) {
    let Some(service_name) = arguments.first().and_then(|value| value.to_str()) else {
        log::error!("Windows SCM did not provide a valid service name");
        return;
    };

    if let Err(e) = run_service(service_name) {
        log::error!("service '{}' exited with error: {:?}", service_name, e);
    }
}

fn run_service(service_name: &str) -> windows_service::Result<()> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let event_handler = move |evt| -> ServiceControlHandlerResult {
        match evt {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle = service_control_handler::register(service_name, event_handler)?;

    // Tell the system that the service is running now
    status_handle.set_service_status(ServiceStatus {
        // Should match the one from system service registry
        service_type: ServiceType::OWN_PROCESS,
        // The new state
        current_state: ServiceState::Running,
        // Accept stop events when running
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        // Used to report an error when starting or stopping only, otherwise must be zero
        exit_code: ServiceExitCode::Win32(0),
        // Only used for pending states, otherwise must be zero
        checkpoint: 0,
        // Only used for pending states, otherwise must be zero
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let config_path = crate::config::resolve_config_path(crate::config::default_config_path());
    let (server_done_tx, server_done_rx) = mpsc::channel::<()>();

    let server_thread = std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::error!("failed to build tokio runtime: {}", e);
                let _ = server_done_tx.send(());
                return;
            }
        };

        if let Err(e) = runtime.block_on(crate::app::run(&config_path.to_string_lossy())) {
            log::error!("supervisord serve exited with error: {}", e);
        }
        let _ = server_done_tx.send(());
    });

    // Wait for either SCM stop or server termination.
    loop {
        if shutdown_rx.recv_timeout(Duration::from_millis(500)).is_ok() {
            break;
        }
        if server_done_rx.try_recv().is_ok() {
            break;
        }
    }

    // The server's tokio runtime runs detached inside server_thread. Abandon
    // it — the process is about to report Stopped and the SCM will terminate
    // us if we linger. Future work: plumb a cancellation signal into
    // serve::run() for a clean teardown of listeners and in-flight queries.
    drop(server_thread);

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

pub fn run_as_service(service_name: &str) -> windows_service::Result<()> {
    service_dispatcher::start(service_name, ffi_service_main)
}
