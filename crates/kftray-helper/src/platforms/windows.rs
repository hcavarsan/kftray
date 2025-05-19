use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(target_os = "windows")]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl,
        ServiceControlAccept,
        ServiceExitCode,
        ServiceState,
        ServiceStatus,
        ServiceType,
    },
    service_control_handler::{
        self,
        ServiceControlHandlerResult,
    },
    service_dispatcher,
};

use crate::{
    address_pool::AddressPoolManager,
    communication::{
        get_default_socket_path,
        start_communication_server,
    },
    error::HelperError,
    network::NetworkConfigManager,
};

pub fn install_service(service_name: &str) -> Result<(), HelperError> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        use windows_service::{
            service::{
                ServiceAccess,
                ServiceErrorControl,
                ServiceInfo,
                ServiceStartType,
            },
            service_manager::{
                ServiceManager,
                ServiceManagerAccess,
            },
        };

        let helper_path = std::env::current_exe().map_err(|e| {
            HelperError::PlatformService(format!("Failed to get current executable path: {}", e))
        })?;

        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to create service manager: {:?}",
                        e
                    ))
                })?;

        let service_info = ServiceInfo {
            name: OsString::from(service_name),
            display_name: OsString::from("KFTray Helper Service"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: helper_path,
            launch_arguments: vec![OsString::from("service")],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };

        let service = manager
            .create_service(&service_info, ServiceAccess::START)
            .map_err(|e| {
                HelperError::PlatformService(format!("Failed to create service: {:?}", e))
            })?;

        service.start::<OsString>(&[]).map_err(|e| {
            HelperError::PlatformService(format!("Failed to start service: {:?}", e))
        })?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

pub fn uninstall_service(service_name: &str) -> Result<(), HelperError> {
    #[cfg(target_os = "windows")]
    {
        use windows_service::{
            service::{
                ServiceAccess,
                ServiceState,
            },
            service_manager::{
                ServiceManager,
                ServiceManagerAccess,
            },
        };

        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|e| {
            HelperError::PlatformService(format!("Failed to connect to service manager: {:?}", e))
        })?;

        let service = manager
            .open_service(
                service_name,
                ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
            )
            .map_err(|e| {
                HelperError::PlatformService(format!("Failed to open service: {:?}", e))
            })?;

        let status = service.query_status().map_err(|e| {
            HelperError::PlatformService(format!("Failed to query service status: {:?}", e))
        })?;

        if status.current_state != ServiceState::Stopped {
            service.stop().map_err(|e| {
                HelperError::PlatformService(format!("Failed to stop service: {:?}", e))
            })?;
        }

        service.delete().map_err(|e| {
            HelperError::PlatformService(format!("Failed to delete service: {:?}", e))
        })?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "windows")]
define_windows_service!(ffi_service_main, kftray_service_main);

pub fn run_service() -> Result<(), HelperError> {
    #[cfg(target_os = "windows")]
    {
        let service_name = "kftray.helper";
        service_dispatcher::start(service_name, ffi_service_main).map_err(|e| {
            HelperError::PlatformService(format!("Failed to start service dispatcher: {:?}", e))
        })?;

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "windows")]
fn kftray_service_main(_arguments: Vec<OsString>) {
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("kftray.helper", event_handler).unwrap();

    let mut status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(0),
        process_id: None,
    };

    status_handle.set_service_status(status.clone()).unwrap();

    match run_service_logic() {
        Ok(_) => {
            status.current_state = ServiceState::Stopped;
            status_handle.set_service_status(status).unwrap();
        }
        Err(e) => {
            status.current_state = ServiceState::Stopped;
            status.exit_code = ServiceExitCode::ServiceSpecific(1);
            status_handle.set_service_status(status).unwrap();
            eprintln!("Service error: {}", e);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_service_logic() -> Result<(), HelperError> {
    println!("Starting helper service on Windows...");

    if tokio::runtime::Handle::try_current().is_ok() {
        println!("Using existing tokio runtime");
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let (pool_manager, network_manager, socket_path) =
                    super::common::initialize_components().await?;

                super::common::run_communication_server(pool_manager, network_manager, socket_path)
                    .await
            })
        })
    } else {
        match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => {
                println!("Successfully created tokio runtime");
                runtime.block_on(async {
                    let (pool_manager, network_manager, socket_path) =
                        super::common::initialize_components().await?;

                    super::common::run_communication_server(
                        pool_manager,
                        network_manager,
                        socket_path,
                    )
                    .await
                })
            }
            Err(e) => {
                eprintln!("Failed to build tokio runtime: {}", e);
                Err(HelperError::PlatformService(format!(
                    "Failed to build tokio runtime: {}",
                    e
                )))
            }
        }
    }
}
