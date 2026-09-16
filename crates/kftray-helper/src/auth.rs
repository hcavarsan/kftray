use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

#[cfg(target_os = "macos")]
use log::warn;
#[cfg(unix)]
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
) -> Result<Option<u32>, HelperError> {
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
            return Ok(Some(cred.pid as u32));
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

    Ok(Some(cred.pid as u32))
}

#[cfg(target_os = "macos")]
pub fn validate_peer_credentials(
    stream: &std::os::unix::net::UnixStream,
) -> Result<Option<u32>, HelperError> {
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
        if cred.cr_uid != authorized_uid {
            return Err(HelperError::Authentication(format!(
                "Peer UID {} is not authorized (expected UID {})",
                cred.cr_uid, authorized_uid
            )));
        }
        info!(
            "Peer credentials validated (root accepting authorized user): UID={}",
            cred.cr_uid
        );
    } else {
        if cred.cr_uid != current_uid {
            return Err(HelperError::Authentication(format!(
                "Peer UID {} does not match expected UID {}",
                cred.cr_uid, current_uid
            )));
        }
        debug!("Peer credentials validated: UID={}", cred.cr_uid);
    }

    // The pid is only used for advisory address-pool ownership tracking,
    // which already treats an unknown pid as "owner unknown" rather than a
    // conflict. A `LOCAL_PEERPID` failure here must not turn an already
    // UID-authorized connection into a hard rejection: peer authentication
    // and pid discovery fail independently.
    match peer_pid(socket_fd) {
        Ok(pid) => Ok(Some(pid)),
        Err(e) => {
            warn!("Could not determine peer pid, proceeding without it: {e}");
            Ok(None)
        }
    }
}

/// The pid of the process on the other end of `socket_fd`.
///
/// macOS's `xucred` carries no pid, unlike Linux's `ucred`: `LOCAL_PEERPID`
/// is a separate `SOL_LOCAL` socket option for it.
#[cfg(target_os = "macos")]
fn peer_pid(socket_fd: std::os::fd::RawFd) -> Result<u32, HelperError> {
    const SOL_LOCAL: libc::c_int = 0;
    const LOCAL_PEERPID: libc::c_int = 0x002;

    let mut pid: libc::pid_t = 0;
    let mut pid_len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket_fd,
            SOL_LOCAL,
            LOCAL_PEERPID,
            &mut pid as *mut _ as *mut libc::c_void,
            &mut pid_len,
        )
    };

    if result != 0 {
        return Err(HelperError::Authentication(
            "Failed to get peer pid".to_string(),
        ));
    }

    Ok(pid as u32)
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
