//! Persists and reads the Windows user account the helper's named pipe
//! trusts.
//!
//! The service runs as `LocalSystem`, so it has no "current user" of its own
//! to compare a connecting client against, and no filesystem object owned by
//! the interactive user the way a Unix socket file is. `record_authorized_user`
//! is called from `install_service`, which runs elevated via UAC but under
//! the real installing account rather than a different one, and persists
//! that account's SID once, machine-wide, so both the pipe's DACL
//! (`create_secure_pipe`) and its client check (`validate_windows_peer`)
//! trust the same identity.

use std::path::PathBuf;

use windows::Win32::Foundation::{
    CloseHandle,
    HANDLE,
    HLOCAL,
    LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW,
    ConvertStringSidToSidW,
};
use windows::Win32::Security::{
    GetTokenInformation,
    PSID,
    TOKEN_QUERY,
    TOKEN_USER,
    TokenUser,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess,
    OpenProcessToken,
};
use windows::core::{
    HSTRING,
    PWSTR,
};

use crate::error::HelperError;

/// A SID is at most 68 bytes; `TokenUser` returns that SID plus one
/// pointer-sized header, comfortably inside this buffer.
const TOKEN_USER_BUF_LEN: usize = 256;

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// A SID allocated by `ConvertStringSidToSidW`, freed with `LocalFree` on
/// drop instead of leaked on every peer check.
pub(crate) struct OwnedSid(pub(crate) PSID);

impl Drop for OwnedSid {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
}

fn authorized_user_sid_path() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("kftray").join("authorized_user.sid")
}

fn auth_err(what: &str, e: windows::core::Error) -> HelperError {
    HelperError::PlatformService(format!("Failed to {what}: {e}"))
}

/// The SID of the account running this process, as an `S-1-...` string.
fn current_process_user_sid_string() -> Result<String, HelperError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| auth_err("open the current process token", e))?;
    }
    let token = OwnedHandle(token);

    let mut buf = [0u8; TOKEN_USER_BUF_LEN];
    let mut returned = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            buf.len() as u32,
            &mut returned,
        )
        .map_err(|e| auth_err("read the current process token SID", e))?;
    }
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };

    let mut sid_string = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut sid_string)
            .map_err(|e| auth_err("stringify the current process SID", e))?;
    }
    let result = unsafe { sid_string.to_string() }
        .map_err(|e| HelperError::PlatformService(format!("SID was not valid UTF-16: {e}")));
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sid_string.as_ptr().cast())));
    }
    result
}

/// Records the SID of the account running this process -- the installing
/// user, elevated via UAC but the same account -- so the service can later
/// verify a connecting pipe client is that same account, and so the pipe's
/// DACL can be scoped to it instead of every local account.
pub fn record_authorized_user() -> Result<(), HelperError> {
    let sid = current_process_user_sid_string()?;
    record_authorized_user_at(&authorized_user_sid_path(), &sid)
}

fn record_authorized_user_at(path: &std::path::Path, sid: &str) -> Result<(), HelperError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(path, sid).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write {}: {e}", path.display()))
    })
}

/// The SID string this helper was told to trust at install, if one was
/// recorded.
pub fn read_authorized_user_sid() -> Option<String> {
    read_authorized_user_sid_at(&authorized_user_sid_path())
}

fn read_authorized_user_sid_at(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The SDDL for the pipe's security descriptor: `Generic All` for SYSTEM,
/// Administrators, and the recorded authorized user, and nothing for anyone
/// else -- an absent DACL entry denies access under NT semantics, so this
/// also rejects every other local or network account.
pub fn pipe_security_descriptor_sddl() -> String {
    format_pipe_sddl(read_authorized_user_sid().as_deref())
}

fn format_pipe_sddl(authorized_sid: Option<&str>) -> String {
    let mut sddl = String::from("D:(A;;GA;;;SY)(A;;GA;;;BA)");
    if let Some(sid) = authorized_sid {
        sddl.push_str(&format!("(A;;GA;;;{sid})"));
    }
    sddl
}

/// Parses an `S-1-...` string into an owned `PSID`.
pub(crate) fn parse_sid(sid_string: &str) -> Result<OwnedSid, HelperError> {
    let wide = HSTRING::from(sid_string);
    let mut sid = PSID::default();
    unsafe {
        ConvertStringSidToSidW(&wide, &mut sid)
            .map_err(|e| auth_err("parse the authorized user SID", e))?;
    }
    Ok(OwnedSid(sid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pipe_sddl_always_grants_system_and_administrators() {
        let sddl = format_pipe_sddl(None);
        assert!(sddl.contains("(A;;GA;;;SY)"));
        assert!(sddl.contains("(A;;GA;;;BA)"));
    }

    #[test]
    fn format_pipe_sddl_adds_the_authorized_user_when_recorded() {
        let sddl = format_pipe_sddl(Some("S-1-5-21-1-2-3-1001"));
        assert!(sddl.contains("(A;;GA;;;S-1-5-21-1-2-3-1001)"));
    }

    #[test]
    fn read_authorized_user_sid_is_none_when_nothing_was_recorded() {
        let path = std::env::temp_dir().join(format!(
            "kftray-sid-missing-test-{}-{}.sid",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);

        assert_eq!(read_authorized_user_sid_at(&path), None);
    }

    #[test]
    fn record_then_read_round_trips_through_the_file() {
        let path = std::env::temp_dir()
            .join(format!(
                "kftray-sid-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("authorized_user.sid");

        record_authorized_user_at(&path, "S-1-5-21-1-2-3-1001").unwrap();
        let read_back = read_authorized_user_sid_at(&path);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        assert_eq!(read_back.as_deref(), Some("S-1-5-21-1-2-3-1001"));
    }

    #[test]
    fn the_current_process_sid_round_trips_through_string_conversion() {
        let sid_string = current_process_user_sid_string().expect("a running process has a SID");
        assert!(
            sid_string.starts_with("S-1-"),
            "unexpected SID format: {sid_string}"
        );

        // Exercises the exact ConvertStringSidToSidW path validate_windows_peer
        // uses to parse the SID recorded at install: if the two Win32 calls
        // disagree on format, every connection would be rejected.
        parse_sid(&sid_string)
            .expect("a SID string produced by ConvertSidToStringSidW must parse back");
    }
}
