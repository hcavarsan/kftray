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

use std::path::{
    Path,
    PathBuf,
};

use log::warn;
use windows::Win32::Foundation::{
    CloseHandle,
    GetLastError,
    HANDLE,
    HLOCAL,
    LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION,
    GetFileSecurityW,
    GetSecurityDescriptorOwner,
    GetTokenInformation,
    OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
    PSID,
    SECURITY_ATTRIBUTES,
    SetFileSecurityW,
    TOKEN_QUERY,
    TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
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

/// Same bytes as `[u8; TOKEN_USER_BUF_LEN]`, but aligned to a pointer
/// boundary so it can be cast to `*const TOKEN_USER`: a plain byte array
/// only guarantees 1-byte alignment, and `TOKEN_USER`'s embedded `PSID`
/// pointer needs pointer alignment on every architecture this helper runs
/// on.
#[repr(C)]
pub(crate) struct AlignedTokenUserBuf {
    _align: [usize; 0],
    bytes: [u8; TOKEN_USER_BUF_LEN],
}

impl AlignedTokenUserBuf {
    pub(crate) const LEN: usize = TOKEN_USER_BUF_LEN;

    pub(crate) fn new() -> Self {
        Self {
            _align: [],
            bytes: [0u8; TOKEN_USER_BUF_LEN],
        }
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }

    /// The `TOKEN_USER` a prior `GetTokenInformation` call into this buffer
    /// wrote, `Sid` included: `GetTokenInformation` appends the SID's bytes
    /// immediately after the header in the same buffer and points `Sid` at
    /// them, so the returned reference must not outlive `self`.
    pub(crate) fn token_user(&self) -> &TOKEN_USER {
        unsafe { &*self.bytes.as_ptr().cast::<TOKEN_USER>() }
    }
}

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

/// A security descriptor allocated by
/// `ConvertStringSecurityDescriptorToSecurityDescriptorW`, freed with
/// `LocalFree` on drop.
struct OwnedDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for OwnedDescriptor {
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

fn last_error(what: &str) -> HelperError {
    let code = unsafe { GetLastError() };
    HelperError::PlatformService(format!("Failed to {what}: OS error {}", code.0))
}

/// The string form of a `PSID`, freeing the string `ConvertSidToStringSidW`
/// allocates once converted.
fn sid_to_string(sid: PSID) -> Result<String, HelperError> {
    let mut sid_string = PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(sid, &mut sid_string).map_err(|e| auth_err("stringify a SID", e))?;
    }
    let result = unsafe { sid_string.to_string() }
        .map_err(|e| HelperError::PlatformService(format!("SID was not valid UTF-16: {e}")));
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sid_string.as_ptr().cast())));
    }
    result
}

/// The SID of the account running this process, as an `S-1-...` string.
///
/// `pub(crate)` so `client::installation` can call it from the unelevated
/// process launching `install`, before UAC elevates a new process that may
/// run as a different administrator account entirely; see
/// `record_authorized_user_from_string`.
pub(crate) fn current_process_user_sid_string() -> Result<String, HelperError> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| auth_err("open the current process token", e))?;
    }
    let token = OwnedHandle(token);

    let mut buf = AlignedTokenUserBuf::new();
    let mut returned = 0u32;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            AlignedTokenUserBuf::LEN as u32,
            &mut returned,
        )
        .map_err(|e| auth_err("read the current process token SID", e))?;
    }

    sid_to_string(buf.token_user().User.Sid)
}

/// Records the SID of the account running this process. Only a fallback
/// for a direct, unelevated `install` invocation: the normal path through
/// `client::installation::install_helper` supplies the caller's own SID
/// via `record_authorized_user_from_string` instead, since by the time
/// this process runs -- after `Start-Process -Verb RunAs` -- it may be
/// running as a different administrator account than the one installing.
pub(crate) fn record_authorized_user() -> Result<(), HelperError> {
    let sid = current_process_user_sid_string()?;
    record_authorized_user_at(&authorized_user_sid_path(), &sid)
}

/// Records `sid` as the authorized user, the same as `record_authorized_user`
/// but for a SID obtained elsewhere: `validate_windows_peer`'s fallback to
/// the active console session, used when an in-place upgrade left no SID
/// recorded yet.
pub(crate) fn record_authorized_user_sid(sid: PSID) -> Result<(), HelperError> {
    let sid_string = sid_to_string(sid)?;
    record_authorized_user_at(&authorized_user_sid_path(), &sid_string)
}

/// Records the SID given as text, the same as `record_authorized_user_sid`
/// but for a SID received as a string: the unelevated process that
/// launches `install`'s elevation computes its own SID with
/// `current_process_user_sid_string` and passes it along, since the
/// elevated process is not guaranteed to run as the same account.
/// Parsing it here rejects a malformed value before it is persisted.
pub(crate) fn record_authorized_user_from_string(sid: &str) -> Result<(), HelperError> {
    let sid = parse_sid(sid)?;
    record_authorized_user_sid(sid.0)
}

fn record_authorized_user_at(path: &Path, sid: &str) -> Result<(), HelperError> {
    if let Some(parent) = path.parent() {
        create_locked_down_dir(parent)?;
    }
    write_locked_down_file(path, sid.as_bytes())
}

/// SDDL granting full control to SYSTEM and Administrators only, and
/// nothing to any other account: an absent DACL entry denies access under
/// NT semantics. The leading `P` marks the DACL protected, so the object
/// does not additionally inherit whatever ACEs its parent directory carries
/// -- `ProgramData` grants far more than this.
const SYSTEM_ADMIN_ONLY_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";

fn locked_down_security_attributes() -> Result<(SECURITY_ATTRIBUTES, OwnedDescriptor), HelperError>
{
    let sddl = HSTRING::from(SYSTEM_ADMIN_ONLY_SDDL);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, 1, &mut descriptor, None)
    }
    .map_err(|e| auth_err("build the SYSTEM/Administrators security descriptor", e))?;

    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    Ok((attrs, OwnedDescriptor(descriptor)))
}

/// Creates `dir` scoped to SYSTEM and Administrators from the moment it
/// exists, instead of with the ACL `CreateDirectoryW` would otherwise
/// inherit from `ProgramData` and locking it down only afterward.
///
/// `ProgramData` grants `Authenticated Users` the right to create
/// subdirectories by default, so an unprivileged process could have
/// pre-created `dir` before install and left it with that permissive
/// inherited ACL. The lockdown is applied even when `dir` already exists,
/// rather than trusting whatever ACL it happens to carry.
fn create_locked_down_dir(dir: &Path) -> Result<(), HelperError> {
    let (attrs, descriptor) = locked_down_security_attributes()?;
    let wide = HSTRING::from(dir.as_os_str());
    if dir.is_dir() {
        let ok = unsafe { SetFileSecurityW(&wide, DACL_SECURITY_INFORMATION, descriptor.0) };
        if !ok.as_bool() {
            return Err(last_error(&format!("secure {}", dir.display())));
        }
        return Ok(());
    }
    unsafe { CreateDirectoryW(&wide, Some(&attrs)) }
        .map_err(|e| auth_err(&format!("create {}", dir.display()), e))
}

/// Writes `contents` to `path`, then narrows its DACL to SYSTEM and
/// Administrators. The file must exist before `SetFileSecurityW` can act on
/// it, so `std::fs::write` runs first; the directory it lands in is already
/// locked down, so the brief window before the DACL narrows still grants
/// nothing beyond SYSTEM and Administrators.
fn write_locked_down_file(path: &Path, contents: &[u8]) -> Result<(), HelperError> {
    std::fs::write(path, contents).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write {}: {e}", path.display()))
    })?;

    let (_attrs, descriptor) = locked_down_security_attributes()?;
    let wide = HSTRING::from(path.as_os_str());
    let ok = unsafe { SetFileSecurityW(&wide, DACL_SECURITY_INFORMATION, descriptor.0) };
    if !ok.as_bool() {
        return Err(last_error(&format!("secure {}", path.display())));
    }
    Ok(())
}

/// The owner SIDs `record_authorized_user_at` is allowed to have left on the
/// file: `LocalSystem`, and `Administrators`, which Windows assigns as the
/// default owner for objects an elevated (UAC split-token) process creates
/// instead of the specific signed-in account.
const TRUSTED_OWNER_SIDS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];

/// The SID string this helper was told to trust at install, if one was
/// recorded.
pub(crate) fn read_authorized_user_sid() -> Option<String> {
    read_authorized_user_sid_at(&authorized_user_sid_path())
}

fn read_authorized_user_sid_at(path: &Path) -> Option<String> {
    let sid = std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;

    match file_owner_sid_string(path) {
        Ok(owner) if TRUSTED_OWNER_SIDS.contains(&owner.as_str()) => Some(sid),
        Ok(owner) => {
            warn!(
                "Refusing to trust {}: owned by {owner}, not SYSTEM or Administrators",
                path.display()
            );
            None
        }
        Err(e) => {
            warn!("Could not verify the owner of {}: {e}", path.display());
            None
        }
    }
}

/// The SID string of `path`'s owner, read straight from the filesystem
/// rather than assumed from how `record_authorized_user_at` wrote it: a
/// file dropped under `ProgramData\kftray` by some other means gains
/// nothing unless it is actually owned by SYSTEM or Administrators.
fn file_owner_sid_string(path: &Path) -> Result<String, HelperError> {
    let wide = HSTRING::from(path.as_os_str());

    let mut needed = 0u32;
    unsafe {
        let _ = GetFileSecurityW(&wide, OWNER_SECURITY_INFORMATION.0, None, 0, &mut needed);
    }
    if needed == 0 {
        return Err(last_error("query the owner security descriptor size"));
    }

    let mut buf = vec![0u8; needed as usize];
    let descriptor = PSECURITY_DESCRIPTOR(buf.as_mut_ptr().cast());
    let mut returned = 0u32;
    let ok = unsafe {
        GetFileSecurityW(
            &wide,
            OWNER_SECURITY_INFORMATION.0,
            Some(descriptor),
            needed,
            &mut returned,
        )
    };
    if !ok.as_bool() {
        return Err(last_error("read the owner security descriptor"));
    }

    let mut owner = PSID::default();
    let mut owner_defaulted = windows::core::BOOL(0);
    unsafe {
        GetSecurityDescriptorOwner(descriptor, &mut owner, &mut owner_defaulted)
            .map_err(|e| auth_err("read the security descriptor owner", e))?;
    }

    sid_to_string(owner)
}

/// The SDDL for the pipe's security descriptor: `Generic All` for SYSTEM,
/// Administrators, and the recorded authorized user, and nothing for anyone
/// else -- an absent DACL entry denies access under NT semantics, so this
/// also rejects every other local or network account.
pub(crate) fn pipe_security_descriptor_sddl() -> String {
    format_pipe_sddl(read_authorized_user_sid().as_deref())
}

/// When no SID has been recorded yet -- a fresh install, or an in-place
/// upgrade from before this file existed -- the DACL additionally grants
/// the Interactive Users group (`IU`, `S-1-5-4`) so the console-session
/// fallback in `validate_windows_peer` is actually reachable: an absent ACE
/// denies access at `CreateFile` time, before that fallback ever runs.
/// Once a specific SID is recorded, access narrows to that account.
fn format_pipe_sddl(authorized_sid: Option<&str>) -> String {
    let mut sddl = String::from("D:(A;;GA;;;SY)(A;;GA;;;BA)");
    match authorized_sid {
        Some(sid) => sddl.push_str(&format!("(A;;GA;;;{sid})")),
        None => sddl.push_str("(A;;GA;;;IU)"),
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
    fn format_pipe_sddl_grants_interactive_users_when_no_sid_recorded() {
        // Without this, an absent ACE denies access under NT semantics and
        // `validate_windows_peer`'s console-session fallback is unreachable:
        // the client is rejected by the kernel before it ever runs.
        let sddl = format_pipe_sddl(None);
        assert!(sddl.contains("(A;;GA;;;IU)"));
    }

    #[test]
    fn format_pipe_sddl_narrows_to_the_recorded_sid_once_one_exists() {
        let sddl = format_pipe_sddl(Some("S-1-5-21-1-2-3-1001"));
        assert!(!sddl.contains("IU"));
    }

    #[test]
    fn format_pipe_sddl_adds_the_authorized_user_when_recorded() {
        let sddl = format_pipe_sddl(Some("S-1-5-21-1-2-3-1001"));
        assert!(sddl.contains("(A;;GA;;;S-1-5-21-1-2-3-1001)"));
    }

    #[test]
    fn record_authorized_user_from_string_rejects_a_malformed_sid_before_persisting_it() {
        // `install_helper` passes whatever `current_process_user_sid_string`
        // returned as a plain CLI argument; a malformed value must be
        // rejected here rather than written verbatim into
        // `authorized_user.sid`.
        let err = record_authorized_user_from_string("not-a-sid").unwrap_err();
        assert!(err.to_string().contains("parse"), "unexpected error: {err}");
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

        // `read_authorized_user_sid_at` additionally requires the file be
        // owned by SYSTEM or Administrators, which a non-elevated test
        // process's own account is not: verified separately below, against
        // `TRUSTED_OWNER_SIDS`, without depending on the account the test
        // runner happens to use. The raw content still round-trips exactly
        // what was written.
        let read_back = std::fs::read_to_string(&path).unwrap();

        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        assert_eq!(read_back.trim(), "S-1-5-21-1-2-3-1001");
    }

    #[test]
    fn only_system_and_administrators_are_trusted_owners() {
        assert!(TRUSTED_OWNER_SIDS.contains(&"S-1-5-18"));
        assert!(TRUSTED_OWNER_SIDS.contains(&"S-1-5-32-544"));
        assert!(!TRUSTED_OWNER_SIDS.contains(&"S-1-5-21-1-2-3-1001"));
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
