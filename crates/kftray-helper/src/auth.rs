use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use log::{
    debug,
    info,
};

use crate::error::HelperError;
use crate::messages::HelperRequest;

const VALID_APP_IDS: &[&str] = &["com.kftray.app", "com.hcavarsan.kftray"];

const MAX_TIMESTAMP_SKEW_SECONDS: u64 = 300;

pub fn validate_request(request: &HelperRequest) -> Result<(), HelperError> {
    validate_app_id(&request.app_id)?;
    validate_timestamp(request.timestamp)?;
    Ok(())
}

fn validate_app_id(app_id: &str) -> Result<(), HelperError> {
    if VALID_APP_IDS.contains(&app_id) {
        Ok(())
    } else {
        Err(HelperError::Authentication(format!(
            "Invalid app_id: {app_id}"
        )))
    }
}

fn validate_timestamp(timestamp: u64) -> Result<(), HelperError> {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HelperError::Authentication("Failed to get current time".to_string()))?
        .as_secs();

    let time_diff = current_time.abs_diff(timestamp);

    if time_diff > MAX_TIMESTAMP_SKEW_SECONDS {
        Err(HelperError::Authentication(format!(
            "Request timestamp too far from current time: {time_diff} seconds"
        )))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub fn validate_peer_credentials(
    stream: &std::os::unix::net::UnixStream,
) -> Result<(), HelperError> {
    use std::os::fd::AsRawFd;

    let socket_fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut cred_len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    let result = unsafe {
        libc::getsockopt(
            socket_fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut cred_len,
        )
    };

    if result != 0 {
        return Err(HelperError::Authentication(
            "Failed to get peer credentials".to_string(),
        ));
    }

    let current_uid = unsafe { libc::getuid() };
    let authorized_uid = get_authorized_user_uid();

    if current_uid == 0 {
        if cred.uid == authorized_uid {
            log::debug!(
                "Peer credentials validated (root accepting authorized user): UID={}, GID={}, PID={}",
                cred.uid, cred.gid, cred.pid
            );
            return Ok(());
        } else {
            return Err(HelperError::Authentication(format!(
                "Peer UID {} is not authorized (expected UID {})",
                cred.uid, authorized_uid
            )));
        }
    }

    if cred.uid != current_uid {
        return Err(HelperError::Authentication(format!(
            "Peer UID {} does not match expected UID {}",
            cred.uid, current_uid
        )));
    }

    log::debug!(
        "Peer credentials validated: UID={}, GID={}, PID={}",
        cred.uid,
        cred.gid,
        cred.pid
    );

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn validate_peer_credentials(
    stream: &std::os::unix::net::UnixStream,
) -> Result<(), HelperError> {
    use std::os::fd::AsRawFd;

    let socket_fd = stream.as_raw_fd();
    let mut cred = libc::xucred {
        cr_version: 0,
        cr_uid: 0,
        cr_ngroups: 0,
        cr_groups: [0; 16],
    };
    let mut cred_len = std::mem::size_of::<libc::xucred>() as libc::socklen_t;

    let result = unsafe {
        libc::getsockopt(
            socket_fd,
            0,
            1,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut cred_len,
        )
    };

    if result != 0 {
        return Err(HelperError::Authentication(
            "Failed to get peer credentials".to_string(),
        ));
    }

    let current_uid = unsafe { libc::getuid() };
    let authorized_uid = get_authorized_user_uid();

    debug!(
        "Current UID: {}, Peer UID: {}, Authorized UID: {}",
        current_uid, cred.cr_uid, authorized_uid
    );

    if current_uid == 0 {
        if cred.cr_uid == authorized_uid {
            info!(
                "Peer credentials validated (root accepting authorized user): UID={}",
                cred.cr_uid
            );
            return Ok(());
        } else {
            return Err(HelperError::Authentication(format!(
                "Peer UID {} is not authorized (expected UID {})",
                cred.cr_uid, authorized_uid
            )));
        }
    }

    if cred.cr_uid != current_uid {
        return Err(HelperError::Authentication(format!(
            "Peer UID {} does not match expected UID {}",
            cred.cr_uid, current_uid
        )));
    }

    log::debug!("Peer credentials validated: UID={}", cred.cr_uid);

    Ok(())
}

#[cfg(unix)]
fn get_authorized_user_uid() -> u32 {
    if let Ok(sudo_uid) = std::env::var("SUDO_UID") {
        if let Ok(uid) = sudo_uid.parse::<u32>() {
            info!("Found authorized UID from SUDO_UID: {uid}");
            return uid;
        }
    }

    if let Ok(socket_path) = crate::communication::get_default_socket_path() {
        if let Ok(metadata) = std::fs::metadata(&socket_path) {
            use std::os::unix::fs::MetadataExt;
            let owner_uid = metadata.uid();
            if owner_uid != 0 {
                info!("Found authorized UID from socket file ownership: {owner_uid}");
                return owner_uid;
            }
        }
    }

    if let Ok(socket_path) = crate::communication::get_default_socket_path() {
        if let Some(parent_dir) = socket_path.parent() {
            if let Ok(metadata) = std::fs::metadata(parent_dir) {
                use std::os::unix::fs::MetadataExt;
                let owner_uid = metadata.uid();
                if owner_uid != 0 {
                    info!("Found authorized UID from socket directory ownership: {owner_uid}");
                    return owner_uid;
                }
            }
        }
    }

    let current_uid = unsafe { libc::getuid() };
    info!("No specific authorized UID found, falling back to current UID: {current_uid}");
    current_uid
}

#[cfg(windows)]
fn get_current_user_sid() -> Result<String, HelperError> {
    use windows::{
        core::PWSTR,
        Win32::Foundation::CloseHandle,
        Win32::Security::Authorization::ConvertSidToStringSidW,
        Win32::Security::{
            GetTokenInformation,
            TokenUser,
            TOKEN_QUERY,
            TOKEN_USER,
        },
        Win32::System::Threading::{
            GetCurrentProcess,
            OpenProcessToken,
        },
    };

    unsafe {
        let process = GetCurrentProcess();
        let mut token = windows::Win32::Foundation::HANDLE::default();

        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|e| {
            HelperError::Authentication(format!("Failed to open process token: {}", e))
        })?;

        let mut token_user_size = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut token_user_size);

        let mut token_user_buffer = vec![0u8; token_user_size as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(token_user_buffer.as_mut_ptr() as *mut _),
            token_user_size,
            &mut token_user_size,
        )
        .map_err(|e| {
            let _ = CloseHandle(token);
            HelperError::Authentication(format!("Failed to get token information: {}", e))
        })?;

        let token_user = &*(token_user_buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_string = PWSTR::null();

        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string).map_err(|e| {
            let _ = CloseHandle(token);
            HelperError::Authentication(format!("Failed to convert SID to string: {}", e))
        })?;

        let sid_str = sid_string.to_string().map_err(|e| {
            let _ = CloseHandle(token);
            HelperError::Authentication(format!("Failed to convert SID to UTF-8: {}", e))
        })?;

        let _ = CloseHandle(token);
        Ok(sid_str)
    }
}

#[cfg(windows)]
fn get_pipe_client_process_id(
    pipe_handle: windows::Win32::Foundation::HANDLE,
) -> Result<u32, HelperError> {
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;

    unsafe {
        let mut client_process_id = 0u32;
        GetNamedPipeClientProcessId(pipe_handle, &mut client_process_id).map_err(|e| {
            HelperError::Authentication(format!("Failed to get client process ID: {}", e))
        })?;
        Ok(client_process_id)
    }
}

#[cfg(windows)]
fn get_process_user_sid(process_id: u32) -> Result<String, HelperError> {
    use windows::{
        core::PWSTR,
        Win32::Foundation::{
            CloseHandle,
            HANDLE,
        },
        Win32::Security::Authorization::ConvertSidToStringSidW,
        Win32::Security::{
            GetTokenInformation,
            TokenUser,
            TOKEN_QUERY,
            TOKEN_USER,
        },
        Win32::System::Threading::{
            OpenProcess,
            OpenProcessToken,
            PROCESS_QUERY_INFORMATION,
        },
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_INFORMATION, false, process_id).map_err(|e| {
            HelperError::Authentication(format!("Failed to open process {}: {}", process_id, e))
        })?;

        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|e| {
            let _ = CloseHandle(process);
            HelperError::Authentication(format!("Failed to open process token: {}", e))
        })?;

        let mut token_user_size = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut token_user_size);

        let mut token_user_buffer = vec![0u8; token_user_size as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(token_user_buffer.as_mut_ptr() as *mut _),
            token_user_size,
            &mut token_user_size,
        )
        .map_err(|e| {
            let _ = CloseHandle(token);
            let _ = CloseHandle(process);
            HelperError::Authentication(format!("Failed to get token information: {}", e))
        })?;

        let token_user = &*(token_user_buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_string = PWSTR::null();

        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string).map_err(|e| {
            let _ = CloseHandle(token);
            let _ = CloseHandle(process);
            HelperError::Authentication(format!("Failed to convert SID to string: {}", e))
        })?;

        let sid_str = sid_string.to_string().map_err(|e| {
            let _ = CloseHandle(token);
            let _ = CloseHandle(process);
            HelperError::Authentication(format!("Failed to convert SID to UTF-8: {}", e))
        })?;

        let _ = CloseHandle(token);
        let _ = CloseHandle(process);
        Ok(sid_str)
    }
}

#[cfg(windows)]
fn get_authorized_user_sid() -> Result<String, HelperError> {
    let current_sid = get_current_user_sid()?;

    if current_sid.starts_with("S-1-5-18") || is_running_as_admin()? {
        if let Ok(pipe_path) = get_default_named_pipe_path() {
            if let Ok(owner_sid) = get_file_owner_sid(&pipe_path) {
                if !owner_sid.starts_with("S-1-5-18") && !is_admin_sid(&owner_sid)? {
                    log::debug!(
                        "Found authorized SID from pipe file ownership: {}",
                        owner_sid
                    );
                    return Ok(owner_sid);
                }
            }
        }

        if let Ok(pipe_path) = get_default_named_pipe_path() {
            if let Some(parent_dir) = std::path::Path::new(&pipe_path).parent() {
                if let Ok(owner_sid) = get_file_owner_sid(parent_dir) {
                    if !owner_sid.starts_with("S-1-5-18") && !is_admin_sid(&owner_sid)? {
                        log::debug!(
                            "Found authorized SID from pipe directory ownership: {}",
                            owner_sid
                        );
                        return Ok(owner_sid);
                    }
                }
            }
        }
    }

    log::debug!("Using current user SID as authorized: {}", current_sid);
    Ok(current_sid)
}

#[cfg(windows)]
fn is_running_as_admin() -> Result<bool, HelperError> {
    use windows::Win32::Foundation::{
        VARIANT_BOOL,
        VARIANT_FALSE,
    };
    use windows::Win32::Security::{
        CheckTokenMembership,
        CreateWellKnownSid,
        WinBuiltinAdministratorsSid,
    };

    unsafe {
        let mut admin_sid_buffer = vec![0u8; 256];
        let mut admin_sid_size = admin_sid_buffer.len() as u32;

        CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            None,
            Some(windows::Win32::Security::PSID(
                admin_sid_buffer.as_mut_ptr() as *mut _,
            )),
            &mut admin_sid_size,
        )
        .map_err(|e| HelperError::Authentication(format!("Failed to create admin SID: {}", e)))?;

        let mut is_member = windows_core::BOOL(0);
        CheckTokenMembership(
            None,
            windows::Win32::Security::PSID(admin_sid_buffer.as_ptr() as *mut _),
            &mut is_member as *mut _,
        )
        .map_err(|e| {
            HelperError::Authentication(format!("Failed to check admin membership: {}", e))
        })?;

        Ok(is_member.as_bool())
    }
}

#[cfg(windows)]
fn is_admin_sid(sid: &str) -> Result<bool, HelperError> {
    Ok(sid.starts_with("S-1-5-32-544")
        || sid.starts_with("S-1-5-18")
        || sid.starts_with("S-1-5-19")
        || sid.starts_with("S-1-5-20"))
}

#[cfg(windows)]
fn get_default_named_pipe_path() -> Result<String, HelperError> {
    use windows::Win32::Storage::FileSystem::GetTempPathW;

    unsafe {
        let mut buffer = vec![0u16; 260];
        let length = GetTempPathW(Some(&mut buffer));
        if length == 0 {
            return Err(HelperError::Authentication(
                "Failed to get temp path".to_string(),
            ));
        }

        buffer.truncate(length as usize);
        let temp_path = String::from_utf16(&buffer)
            .map_err(|_| HelperError::Authentication("Invalid temp path".to_string()))?;

        Ok(format!("{}kftray-helper-pipe", temp_path))
    }
}

#[cfg(windows)]
fn get_file_owner_sid<P: AsRef<std::path::Path>>(path: P) -> Result<String, HelperError> {
    use windows::{
        core::PWSTR,
        Win32::Foundation::LocalFree,
        Win32::Security::Authorization::ConvertSidToStringSidW,
        Win32::Security::{
            GetFileSecurityW,
            GetSecurityDescriptorOwner,
            OWNER_SECURITY_INFORMATION,
        },
    };

    let path_str = path.as_ref().to_string_lossy();
    let path_wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut size_needed = 0u32;
        let _ = GetFileSecurityW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            OWNER_SECURITY_INFORMATION.0,
            None,
            0,
            &mut size_needed,
        );

        let mut security_descriptor = vec![0u8; size_needed as usize];
        let result = GetFileSecurityW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            OWNER_SECURITY_INFORMATION.0,
            Some(windows::Win32::Security::PSECURITY_DESCRIPTOR(
                security_descriptor.as_mut_ptr() as *mut _,
            )),
            size_needed,
            &mut size_needed,
        );
        if result.as_bool() == false {
            return Err(HelperError::Authentication(
                "Failed to get file security".to_string(),
            ));
        }

        let mut owner_sid = std::ptr::null_mut();
        let mut owner_defaulted = windows_core::BOOL::from(false);

        GetSecurityDescriptorOwner(
            windows::Win32::Security::PSECURITY_DESCRIPTOR(security_descriptor.as_ptr() as *mut _),
            owner_sid,
            &mut owner_defaulted,
        )
        .map_err(|e| {
            HelperError::Authentication(format!(
                "Failed to get owner from security descriptor: {}",
                e
            ))
        })?;

        let mut sid_string = PWSTR::null();
        ConvertSidToStringSidW(
            windows::Win32::Security::PSID(owner_sid as *mut _),
            &mut sid_string,
        )
        .map_err(|e| {
            HelperError::Authentication(format!("Failed to convert owner SID to string: {}", e))
        })?;

        let result = sid_string.to_string().map_err(|e| {
            HelperError::Authentication(format!("Failed to convert SID to UTF-8: {}", e))
        })?;

        LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            sid_string.0 as *mut _,
        )));
        Ok(result)
    }
}

#[cfg(windows)]
pub fn validate_peer_credentials(
    pipe_handle: windows::Win32::Foundation::HANDLE,
) -> Result<(), HelperError> {
    let client_process_id = get_pipe_client_process_id(pipe_handle)?;
    log::debug!("Client process ID: {}", client_process_id);

    let client_sid = get_process_user_sid(client_process_id)?;
    log::debug!("Client user SID: {}", client_sid);

    let authorized_sid = get_authorized_user_sid()?;
    log::debug!("Authorized user SID: {}", authorized_sid);

    if client_sid == authorized_sid {
        log::debug!("Peer credentials validated: SID matches");
        Ok(())
    } else {
        Err(HelperError::Authentication(format!(
            "Peer SID {} does not match authorized SID {}",
            client_sid, authorized_sid
        )))
    }
}

#[cfg(not(any(unix, windows)))]
fn get_authorized_user_uid() -> u32 {
    warn!("Unsupported platform: No UID-based authorization, using default");
    0
}

#[cfg(not(any(unix, windows)))]
pub fn validate_peer_credentials<T>(_stream: &T) -> Result<(), HelperError> {
    warn!("Unsupported platform: Peer credential validation skipped");
    Ok(())
}
