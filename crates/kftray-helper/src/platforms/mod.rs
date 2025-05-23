use crate::error::HelperError;

mod common;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

pub fn install_platform_service(service_name: &str) -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        macos::install_service(service_name)
    }

    #[cfg(target_os = "linux")]
    {
        linux::install_service(service_name)
    }

    #[cfg(target_os = "windows")]
    {
        windows::install_service(service_name)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

pub fn uninstall_platform_service(service_name: &str) -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        macos::uninstall_service(service_name)
    }

    #[cfg(target_os = "linux")]
    {
        linux::uninstall_service(service_name)
    }

    #[cfg(target_os = "windows")]
    {
        windows::uninstall_service(service_name)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}

pub fn run_platform_service() -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        macos::run_service()
    }

    #[cfg(target_os = "linux")]
    {
        linux::run_service()
    }

    #[cfg(target_os = "windows")]
    {
        windows::run_service()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(HelperError::UnsupportedPlatform)
    }
}
