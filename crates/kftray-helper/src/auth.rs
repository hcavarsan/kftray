use std::time::{
    SystemTime,
    UNIX_EPOCH,
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
            "Invalid app_id: {}",
            app_id
        )))
    }
}

fn validate_timestamp(timestamp: u64) -> Result<(), HelperError> {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HelperError::Authentication("Failed to get current time".to_string()))?
        .as_secs();

    let time_diff = if current_time > timestamp {
        current_time - timestamp
    } else {
        timestamp - current_time
    };

    if time_diff > MAX_TIMESTAMP_SKEW_SECONDS {
        Err(HelperError::Authentication(format!(
            "Request timestamp too far from current time: {} seconds",
            time_diff
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

    println!(
        "Current UID: {}, Peer UID: {}, Authorized UID: {}",
        current_uid, cred.cr_uid, authorized_uid
    );

    if current_uid == 0 {
        if cred.cr_uid == authorized_uid {
            println!(
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
            println!("Found authorized UID from SUDO_UID: {}", uid);
            return uid;
        }
    }

    if let Ok(socket_path) = crate::communication::get_default_socket_path() {
        if let Ok(metadata) = std::fs::metadata(&socket_path) {
            use std::os::unix::fs::MetadataExt;
            let owner_uid = metadata.uid();
            if owner_uid != 0 {
                println!(
                    "Found authorized UID from socket file ownership: {}",
                    owner_uid
                );
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
                    println!(
                        "Found authorized UID from socket directory ownership: {}",
                        owner_uid
                    );
                    return owner_uid;
                }
            }
        }
    }

    let current_uid = unsafe { libc::getuid() };
    println!(
        "No specific authorized UID found, falling back to current UID: {}",
        current_uid
    );
    current_uid
}

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

#[cfg(windows)]
fn get_current_user_sid() -> Result<String, HelperError> {
    use std::ptr;

    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{
        GetCurrentProcess,
        OpenProcessToken,
    };
    use winapi::um::sddl::ConvertSidToStringSidW;
    use winapi::um::securitybaseapi::GetTokenInformation;
    use winapi::um::winbase::LocalFree;
    use winapi::um::winnt::{
        TokenUser,
        PSID,
        TOKEN_QUERY,
        TOKEN_USER,
    };

    unsafe {
        let mut token = ptr::null_mut();
        let process = GetCurrentProcess();

        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(HelperError::Authentication(
                "Failed to open process token".to_string(),
            ));
        }

        let mut token_user_size = 0u32;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut token_user_size);

        let mut token_user_buffer = vec![0u8; token_user_size as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            token_user_buffer.as_mut_ptr() as *mut _,
            token_user_size,
            &mut token_user_size,
        ) == 0
        {
            CloseHandle(token);
            return Err(HelperError::Authentication(
                "Failed to get token information".to_string(),
            ));
        }

        let token_user = &*(token_user_buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_string = ptr::null_mut();

        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string) == 0 {
            CloseHandle(token);
            return Err(HelperError::Authentication(
                "Failed to convert SID to string".to_string(),
            ));
        }

        let sid_slice = std::slice::from_raw_parts(
            sid_string,
            (0..).take_while(|&i| *sid_string.offset(i) != 0).count() + 1,
        );
        let sid_os_string = OsString::from_wide(&sid_slice[..sid_slice.len() - 1]);
        let sid_string_result = sid_os_string.to_string_lossy().to_string();

        LocalFree(sid_string as *mut _);
        CloseHandle(token);

        Ok(sid_string_result)
    }
}

#[cfg(windows)]
fn get_pipe_client_pid(pipe_handle: std::os::windows::io::RawHandle) -> Result<u32, HelperError> {
    use winapi::um::namedpipeapi::GetNamedPipeClientProcessId;

    unsafe {
        let mut client_pid = 0u32;
        if GetNamedPipeClientProcessId(pipe_handle as *mut _, &mut client_pid) == 0 {
            return Err(HelperError::Authentication(
                "Failed to get client process ID".to_string(),
            ));
        }
        Ok(client_pid)
    }
}

#[cfg(windows)]
fn get_process_user_sid(pid: u32) -> Result<String, HelperError> {
    use std::ptr;

    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::processthreadsapi::OpenProcessToken;
    use winapi::um::sddl::ConvertSidToStringSidW;
    use winapi::um::securitybaseapi::GetTokenInformation;
    use winapi::um::winbase::LocalFree;
    use winapi::um::winnt::{
        TokenUser,
        PROCESS_QUERY_INFORMATION,
        TOKEN_QUERY,
        TOKEN_USER,
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(HelperError::Authentication(format!(
                "Failed to open process {}",
                pid
            )));
        }

        let mut token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            CloseHandle(process);
            return Err(HelperError::Authentication(
                "Failed to open process token".to_string(),
            ));
        }

        let mut token_user_size = 0u32;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut token_user_size);

        let mut token_user_buffer = vec![0u8; token_user_size as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            token_user_buffer.as_mut_ptr() as *mut _,
            token_user_size,
            &mut token_user_size,
        ) == 0
        {
            CloseHandle(token);
            CloseHandle(process);
            return Err(HelperError::Authentication(
                "Failed to get token information".to_string(),
            ));
        }

        let token_user = &*(token_user_buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_string = ptr::null_mut();

        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string) == 0 {
            CloseHandle(token);
            CloseHandle(process);
            return Err(HelperError::Authentication(
                "Failed to convert SID to string".to_string(),
            ));
        }

        let sid_slice = std::slice::from_raw_parts(
            sid_string,
            (0..).take_while(|&i| *sid_string.offset(i) != 0).count() + 1,
        );
        let sid_os_string = OsString::from_wide(&sid_slice[..sid_slice.len() - 1]);
        let sid_string_result = sid_os_string.to_string_lossy().to_string();

        LocalFree(sid_string as *mut _);
        CloseHandle(token);
        CloseHandle(process);

        Ok(sid_string_result)
    }
}

#[cfg(windows)]
fn get_authorized_user_sid() -> Result<String, HelperError> {
    let current_sid = get_current_user_sid()?;
    log::debug!("Using current user SID as authorized: {}", current_sid);
    Ok(current_sid)
}

#[cfg(windows)]
pub fn validate_peer_credentials(
    pipe_handle: std::os::windows::io::RawHandle,
) -> Result<(), HelperError> {
    let client_pid = get_pipe_client_pid(pipe_handle)?;
    log::debug!("Client process ID: {}", client_pid);

    let client_sid = get_process_user_sid(client_pid)?;
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
    println!("Unsupported platform: No UID-based authorization, using default");
    0
}

#[cfg(not(any(unix, windows)))]
pub fn validate_peer_credentials<T>(_stream: &T) -> Result<(), HelperError> {
    println!("Unsupported platform: Peer credential validation skipped");
    Ok(())
}
