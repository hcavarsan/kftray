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
            debug!(
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

    debug!(
        "Peer credentials validated: UID={}, GID={}, PID={}",
        cred.uid, cred.gid, cred.pid
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

    debug!("Peer credentials validated: UID={}", cred.cr_uid);

    Ok(())
}

#[cfg(unix)]
fn get_authorized_user_uid() -> u32 {
    if let Ok(sudo_uid) = std::env::var("SUDO_UID")
        && let Ok(uid) = sudo_uid.parse::<u32>()
    {
        info!("Found authorized UID from SUDO_UID: {uid}");
        return uid;
    }

    if let Ok(socket_path) = crate::communication::get_default_socket_path()
        && let Ok(metadata) = std::fs::metadata(&socket_path)
    {
        use std::os::unix::fs::MetadataExt;
        let owner_uid = metadata.uid();
        if owner_uid != 0 {
            info!("Found authorized UID from socket file ownership: {owner_uid}");
            return owner_uid;
        }
    }

    if let Ok(socket_path) = crate::communication::get_default_socket_path()
        && let Some(parent_dir) = socket_path.parent()
        && let Ok(metadata) = std::fs::metadata(parent_dir)
    {
        use std::os::unix::fs::MetadataExt;
        let owner_uid = metadata.uid();
        if owner_uid != 0 {
            info!("Found authorized UID from socket directory ownership: {owner_uid}");
            return owner_uid;
        }
    }

    let current_uid = unsafe { libc::getuid() };
    info!("No specific authorized UID found, falling back to current UID: {current_uid}");
    current_uid
}

#[cfg(windows)]
pub fn validate_peer_credentials(
    _pipe_handle: windows::Win32::Foundation::HANDLE,
) -> Result<(), HelperError> {
    Ok(())
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
