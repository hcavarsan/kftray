use std::{
    env,
    fs,
    path::PathBuf,
};

use anyhow::Result;

pub fn create_config_dir() -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = get_config_dir()?;
    fs::create_dir_all(config_dir)?;
    Ok(())
}

pub fn get_config_dir() -> Result<PathBuf, String> {
    if let Ok(config_dir) = env::var("KFTRAY_CONFIG") {
        return Ok(PathBuf::from(config_dir));
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        let mut path = PathBuf::from(xdg_config_home);
        path.push("kftray");
        return Ok(path);
    }

    if let Some(home_dir) = dirs::home_dir() {
        let mut path = home_dir;
        path.push(".kftray");
        return Ok(path);
    }

    Err("Unable to determine the configuration directory".to_string())
}

pub fn get_log_folder_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("http_logs");
    Ok(config_path)
}

/// Identifier for this kftray installation, persisted next to the database.
///
/// Config ids come from a local database, so two installations can hold the
/// same id and their cluster resources are otherwise indistinguishable. This
/// value labels the resources one installation owns.
///
/// It fails rather than falling back to a shared placeholder: an identifier two
/// installations could both produce would let one delete the other's proxies.
/// Only a success is cached: a transient filesystem error or lock timeout would
/// otherwise disable proxy ownership for the rest of the process even after the
/// cause cleared.
///
/// Creation touches the filesystem and can wait on the lock, so it runs on a
/// blocking thread rather than stalling a runtime worker and delaying unrelated
/// forwards. A cached identifier is returned without leaving the runtime.
pub async fn installation_id() -> Result<&'static str, String> {
    if let Some(id) = INSTALLATION_ID.get() {
        return Ok(id);
    }
    let id = tokio::task::spawn_blocking(load_or_create_installation_id)
        .await
        .map_err(|error| format!("Installation identifier task failed: {error}"))??;

    Ok(INSTALLATION_ID.get_or_init(|| id))
}

static INSTALLATION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The identity that owns cluster resources created from one database.
///
/// The file database is one per installation, so its identity is the
/// installation's. An in-memory database exists only in one process and is
/// independent of every other, including the file database of the same
/// installation, and the two allocate the same configuration ids; without a
/// distinct identity a memory-mode run would find, and clean up, the file
/// database's resources for the same id. The memory identity extends the
/// installation's with a session identifier, so the resources screen can
/// still recognise both as this installation's.
pub async fn owner_identity(mode: crate::utils::db_mode::DatabaseMode) -> Result<String, String> {
    let installation = installation_id().await?;
    Ok(match mode {
        crate::utils::db_mode::DatabaseMode::File => installation.to_owned(),
        crate::utils::db_mode::DatabaseMode::Memory => format!(
            "{}{MEMORY_OWNER_SEPARATOR}{}",
            memory_owner_base(installation),
            MEMORY_DATABASE_IDENTITY.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
        ),
    })
}

/// Separates the installation's part of a memory-mode identity from the
/// session's.
const MEMORY_OWNER_SEPARATOR: &str = "-m";

/// Marks the digest form of a memory-mode owner base.
///
/// A generated installation id is lowercase hex, so it can never contain
/// this letter; [`is_valid_installation_id`] also rejects a persisted id
/// that starts with it. That makes a digest form unmistakable for a real
/// installation id, so [`owned_by_installation`] cannot recognise another
/// installation's raw id as this installation's digest.
const MEMORY_OWNER_DIGEST_PREFIX: char = 'h';

/// The part of the installation id a memory-mode identity is derived from.
///
/// The identity is a label value, so the whole derived form has to fit the
/// 63-character limit with a full 128-bit session identifier after it; two
/// sessions of one installation must not be able to collide on the resources
/// they select by configuration id. `load_or_create_installation_id` always
/// persists a 32-character UUID, longer than `BASE_LEN`, so the digest below
/// is the path every installation actually takes, not a rare fallback for
/// unusually long ids: two installations whose digests collide would each
/// recognise the other's memory-session resources as their own, so the
/// digest is widened to use the rest of the `BASE_LEN` budget.
fn memory_owner_base(installation: &str) -> std::borrow::Cow<'_, str> {
    const SUFFIX_LEN: usize = MEMORY_OWNER_SEPARATOR.len() + 32;
    const BASE_LEN: usize = 63 - SUFFIX_LEN;
    if installation.len() <= BASE_LEN {
        return std::borrow::Cow::Borrowed(installation);
    }
    std::borrow::Cow::Owned(format!(
        "{MEMORY_OWNER_DIGEST_PREFIX}{}",
        fnv1a_hex(installation.bytes())
    ))
}

/// FNV-1a over `bytes`, fixed here rather than left to the standard
/// hasher.
///
/// A value derived from this can be written to disk, or into a label, by
/// one process and read back by another built at a different time; it must
/// not depend on whatever `DefaultHasher` happens to do for a given Rust
/// version. Two independently-seeded passes are combined into 112 bits (28
/// hex characters, the most `memory_owner_base`'s `BASE_LEN` budget can
/// hold) rather than a single 64-bit hash, to keep a cross-installation
/// collision implausible.
pub(crate) fn fnv1a_hex(bytes: impl IntoIterator<Item = u8>) -> String {
    let data: Vec<u8> = bytes.into_iter().collect();
    let low = fnv1a_u64(&data, 0xcbf2_9ce4_8422_2325);
    let high = fnv1a_u64(&data, 0x9e37_79b9_7f4a_7c15);
    format!("{low:016x}{:012x}", high & 0xffff_ffff_ffff)
}

fn fnv1a_u64(bytes: &[u8], offset_basis: u64) -> u64 {
    let mut hash = offset_basis;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Whether an ownership label names this installation, directly or through
/// one of its memory-mode sessions.
pub fn owned_by_installation(owner: &str, installation: &str) -> bool {
    owner == installation
        || owner
            .strip_prefix(memory_owner_base(installation).as_ref())
            .and_then(|rest| rest.strip_prefix(MEMORY_OWNER_SEPARATOR))
            .is_some_and(|session| {
                session.len() == 32 && session.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

static MEMORY_DATABASE_IDENTITY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn load_or_create_installation_id() -> Result<String, String> {
    let config_dir = get_config_dir()?;
    let path = config_dir.join("installation_id");

    fs::create_dir_all(&config_dir).map_err(|error| {
        format!(
            "Failed to create the configuration directory {}: {error}",
            config_dir.display()
        )
    })?;

    // Reading happens under the same lock as creation: a publisher can be
    // between its rename and its directory sync, and adopting an identifier
    // that is not yet durable would make the next launch generate a different
    // one and stop matching the resources this one labelled.
    with_identity_lock(&config_dir, || {
        let stored = match fs::read_to_string(&path) {
            Ok(stored) => stored.trim().to_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(format!(
                    "Failed to read the installation identifier at {}: {error}",
                    path.display()
                ));
            }
        };
        if is_valid_installation_id(&stored) {
            // Durability matters when publishing a new identifier, not when
            // merely reading one back: a filesystem that rejects fsync on a
            // read-only directory descriptor (some network or container
            // mounts) must not stop this installation from ever starting.
            if let Err(error) = sync_directory(&config_dir) {
                log::warn!(
                    "Failed to synchronize {} before using its installation identifier: {error}",
                    config_dir.display()
                );
            }

            return Ok(stored);
        }
        if !stored.is_empty() {
            log::warn!(
                "Discarding an invalid installation identifier read from {}: {stored:?}",
                path.display()
            );
        }
        let generated = uuid::Uuid::new_v4().simple().to_string();
        publish_installation_id(&path, &generated)?;
        Ok(generated)
    })
}

/// Runs `write` while holding an exclusive advisory lock on a file next to the
/// identifier.
///
/// The lock is held by the process through an open descriptor, so it is
/// released by the kernel when that process exits. A holder that is merely slow
/// keeps its lock, which an age heuristic could not distinguish from a crash.
fn with_identity_lock<T>(
    config_dir: &std::path::Path, write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    with_file_lock(&config_dir.join("installation_id.lock"), write)
}

/// Releases the lock when dropped, so [`with_file_lock`] unlocks on every
/// exit path `work` can take, a panic included, not only its normal return.
struct FileLockGuard<'a> {
    file: &'a fs::File,
}

impl Drop for FileLockGuard<'_> {
    fn drop(&mut self) {
        unlock(self.file, LockRegion::Whole);
    }
}

/// Runs `work` while holding an exclusive advisory lock on `lock_path`.
///
/// The lock is held through an open descriptor, so the kernel releases it when
/// the process exits. Every process that takes the same path serializes against
/// the others, which is what makes a read-modify-write of a shared file safe.
pub fn with_file_lock<T>(
    lock_path: &std::path::Path, work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create the lock directory at {}: {error}",
                parent.display()
            )
        })?;
    }
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| {
            format!(
                "Failed to open the lock at {}: {error}",
                lock_path.display()
            )
        })?;

    wait_for_exclusive_lock(&lock, LockRegion::Whole, &lock_path.display().to_string())?;
    let _guard = FileLockGuard { file: &lock };
    work()
}

/// A lock on one configuration, shared by every process that uses the file
/// database.
///
/// Starting, stopping and deleting a configuration each hold it for the
/// whole operation, so a delete in one process cannot slip between another
/// process's existence check and its registration, and two processes cannot
/// tear down or set up the same row at once. The in-memory database belongs
/// to one process, whose own lifecycle lock is enough; no file lock is taken
/// for it. Released when dropped, and by the kernel if the process dies.
pub struct ConfigLock {
    file: fs::File,
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        unlock(&self.file, LockRegion::Whole);
    }
}

/// Takes the cross-process lock for `id`, waiting up to `budget` for another
/// process to finish with it.
///
/// The wait runs on a blocking thread: it can sleep for the whole budget, and
/// a runtime worker parked on it would stall unrelated forwards.
///
/// The lock file this creates under `locks/` is never removed. Unlinking it
/// while another process still holds it open would let a third process
/// create and lock a new inode at the same path, so the two existing holders
/// would no longer exclude each other. One small file per configuration id
/// is an acceptable, bounded cost next to that risk.
pub async fn lock_config(
    id: i64, mode: crate::utils::db_mode::DatabaseMode, budget: std::time::Duration,
) -> Result<Option<ConfigLock>, String> {
    if mode != crate::utils::db_mode::DatabaseMode::File {
        return Ok(None);
    }
    let lock_path = get_config_dir()?
        .join("locks")
        .join(format!("config-{id}.lock"));
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create the lock directory at {}: {error}",
                    parent.display()
                )
            })?;
        }
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                format!(
                    "Failed to open the lock at {}: {error}",
                    lock_path.display()
                )
            })?;
        wait_for_exclusive_lock_within(
            &file,
            LockRegion::Whole,
            &format!("config {id} (another kftray process may be working on it)"),
            budget,
        )?;
        Ok(Some(ConfigLock { file }))
    })
    .await
    .map_err(|error| format!("Lock task failed: {error}"))?
}

/// Which bytes of a file an exclusive lock covers.
///
/// Advisory `flock` locks cover the whole file whatever the region. Windows
/// locks are mandatory and enforced against every other handle, including the
/// holder's own, so a file that the holder also has to rewrite is locked on
/// a byte far beyond any length it will ever have instead.
#[derive(Clone, Copy)]
pub(crate) enum LockRegion {
    Whole,
    /// A single byte at a 1 GiB offset, in the manner of SQLite's pending byte.
    PendingByte,
}

/// Waits for an exclusive lock on `file`, up to a fixed budget.
///
/// `what` names the file in errors.
pub(crate) fn wait_for_exclusive_lock(
    file: &fs::File, region: LockRegion, what: &str,
) -> Result<(), String> {
    wait_for_exclusive_lock_within(file, region, what, std::time::Duration::from_secs(15))
}

/// Waits for an exclusive lock on `file`, up to `budget`.
pub(crate) fn wait_for_exclusive_lock_within(
    file: &fs::File, region: LockRegion, what: &str, budget: std::time::Duration,
) -> Result<(), String> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(25);

    let deadline = std::time::Instant::now() + budget;
    loop {
        match try_lock_exclusive(file, region) {
            Ok(true) => return Ok(()),
            // Only contention is worth waiting out. A filesystem without
            // advisory locking refuses every attempt, and waiting the full
            // budget would report a timeout instead of the reason.
            Ok(false) => {}
            Err(error) => {
                return Err(format!("Failed to take the lock at {what}: {error}"));
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("Timed out waiting for the lock at {what}"));
        }
        std::thread::sleep(POLL);
    }
}

/// Takes the lock, reporting contention as `Ok(false)` and anything else as an
/// error: a filesystem that cannot lock at all must not look like a busy peer.
#[cfg(unix)]
fn try_lock_exclusive(file: &fs::File, _region: LockRegion) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor is owned by `file` and outlives this call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(false),
        // Retried rather than reported: a signal interrupted the call, which
        // says nothing about the lock.
        Some(libc::EINTR) => Ok(false),
        _ => Err(error),
    }
}

#[cfg(unix)]
pub(crate) fn unlock(file: &fs::File, _region: LockRegion) {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor is owned by `file` and outlives this call.
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

/// Offset and length of the bytes a region locks.
#[cfg(windows)]
fn region_bytes(region: LockRegion) -> (u64, u32, u32) {
    match region {
        LockRegion::Whole => (0, u32::MAX, u32::MAX),
        LockRegion::PendingByte => (1 << 30, 1, 0),
    }
}

#[cfg(windows)]
fn overlapped_at(offset: u64) -> windows::Win32::System::IO::OVERLAPPED {
    let mut overlapped = windows::Win32::System::IO::OVERLAPPED::default();
    overlapped.Anonymous.Anonymous.Offset = offset as u32;
    overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
    overlapped
}

#[cfg(windows)]
fn try_lock_exclusive(file: &fs::File, region: LockRegion) -> std::io::Result<bool> {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY,
        LockFileEx,
    };

    let (offset, low, high) = region_bytes(region);
    let mut overlapped = overlapped_at(offset);

    // SAFETY: the handle is owned by `file` and outlives this call.
    let locked = unsafe {
        LockFileEx(
            HANDLE(file.as_raw_handle()),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            None,
            low,
            high,
            &mut overlapped,
        )
    };
    match locked {
        Ok(()) => Ok(true),
        // Compared as an HRESULT: `Error::code` wraps the Win32 status, so the
        // raw ERROR_LOCK_VIOLATION value never matches and contention would be
        // reported as a hard failure instead of reaching the retry loop.
        Err(error)
            if error.code()
                == windows::core::HRESULT::from_win32(
                    windows::Win32::Foundation::ERROR_LOCK_VIOLATION.0,
                ) =>
        {
            Ok(false)
        }
        Err(error) => Err(std::io::Error::other(error)),
    }
}

#[cfg(windows)]
pub(crate) fn unlock(file: &fs::File, region: LockRegion) {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::UnlockFileEx;

    let (offset, low, high) = region_bytes(region);
    let mut overlapped = overlapped_at(offset);

    // SAFETY: the handle is owned by `file` and outlives this call.
    unsafe {
        let _ = UnlockFileEx(
            HANDLE(file.as_raw_handle()),
            None,
            low,
            high,
            &mut overlapped,
        );
    }
}

static TEMP_FILE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Writes `contents` to `path` via a temporary file in the same directory,
/// fsyncing the file and the directory entry before returning, so a reader
/// never observes the destination path empty or partially written.
pub(crate) fn write_file_durably(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path has no parent directory",
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let unique = TEMP_FILE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = dir.join(format!("{file_name}.{}.{unique}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        durable_rename(&temporary, path)?;
        // The rename itself has to reach disk before a reader can observe
        // the new contents: losing the directory entry would leave the
        // previous file in place, or none at all.
        sync_directory(dir)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Writes the installation identifier in full via [`write_file_durably`], so
/// the published path is never visible empty.
fn publish_installation_id(path: &std::path::Path, id: &str) -> Result<(), String> {
    write_file_durably(path, id.as_bytes()).map_err(|error| {
        format!(
            "Failed to persist the installation identifier at {}: {error}",
            path.display()
        )
    })
}

/// Moves `from` onto `to` so the rename itself is durable.
#[cfg(unix)]
fn durable_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

/// Windows has no directory to sync, so durability is requested from the move
/// itself: a plain rename can be acknowledged before it reaches the disk.
#[cfg(windows)]
fn durable_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING,
        MOVEFILE_WRITE_THROUGH,
        MoveFileExW,
    };
    use windows::core::PCWSTR;

    let wide = |path: &std::path::Path| -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    };
    let from = wide(from);
    let to = wide(to);

    // SAFETY: both strings are NUL-terminated and live across the call.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

#[cfg(unix)]
fn sync_directory(path: &std::path::Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(path: &std::path::Path) -> std::io::Result<()> {
    // Windows has no directory handle to sync; the rename is durable once the
    // file's own data has been flushed.
    let _ = path;
    Ok(())
}

fn is_valid_installation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && !value.starts_with(MEMORY_OWNER_DIGEST_PREFIX)
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

pub fn get_db_file_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("configs.db");
    Ok(config_path)
}

pub fn get_pod_manifest_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("proxy_manifest.json");
    Ok(config_path)
}

pub fn get_proxy_deployment_manifest_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("proxy_deployment.json");
    Ok(config_path)
}

pub fn get_expose_deployment_manifest_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("expose_deployment.json");
    Ok(config_path)
}

pub fn get_expose_service_manifest_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("expose_service.json");
    Ok(config_path)
}

pub fn get_expose_ingress_manifest_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("expose_ingress.json");
    Ok(config_path)
}

pub fn get_app_log_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("app.log");
    Ok(config_path)
}

pub fn get_window_state_path() -> Result<PathBuf, String> {
    let mut config_path = get_config_dir()?;
    config_path.push("window_position.json");
    Ok(config_path)
}

pub fn get_kubeconfig_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    if let Ok(kubeconfig_paths) = env::var("KUBECONFIG") {
        for path in kubeconfig_paths.split(if cfg!(windows) { ';' } else { ':' }) {
            let path_buf = PathBuf::from(path);
            if path_buf.exists() {
                paths.push(path_buf);
            }
        }
    }

    if paths.is_empty()
        && let Some(mut config_path) = dirs::home_dir()
    {
        config_path.push(".kube/config");
        if config_path.exists() {
            paths.push(config_path);
        }
    }

    if paths.is_empty() {
        Err(anyhow::anyhow!("Unable to determine kubeconfig path"))
    } else {
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use lazy_static::lazy_static;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_memory_session_of_a_long_installation_id_is_still_recognised_as_its_own() {
        let installation = "a".repeat(63);
        let session = uuid::Uuid::new_v4().simple().to_string();
        let owner = format!("{}-m{session}", memory_owner_base(&installation));
        assert!(
            owner.len() <= 63,
            "the derived identity must fit a label value"
        );
        assert!(owned_by_installation(&owner, &installation));
        assert!(owned_by_installation(&installation, &installation));
        assert!(!owned_by_installation(
            &format!("{}-m{session}", memory_owner_base(&"b".repeat(63))),
            &installation
        ));
        // Two long ids that share every character the base could have kept.
        let sibling = format!("{}b", &installation[..62]);
        assert!(!owned_by_installation(
            &format!("{}-m{session}", memory_owner_base(&sibling)),
            &installation
        ));
        assert!(
            !owned_by_installation(&format!("{installation}-m"), &installation),
            "a bare separator is not a session"
        );
    }

    #[test]
    fn a_memory_session_of_a_full_length_generated_installation_id_fits_under_63_chars() {
        // What `load_or_create_installation_id` actually persists: a full
        // 32-hex-character UUID, not a truncated prefix of one.
        let installation = uuid::Uuid::new_v4().simple().to_string();
        assert_eq!(installation.len(), 32);
        let session = uuid::Uuid::new_v4().simple().to_string();
        let owner = format!("{}-m{session}", memory_owner_base(&installation));
        assert!(
            owner.len() <= 63,
            "a full-length installation id's memory owner must still fit a label value: {owner}"
        );
        assert!(owned_by_installation(&owner, &installation));
    }

    #[test]
    fn a_session_that_is_not_hex_is_not_recognised_even_at_the_right_length() {
        let installation = "abc123abc123".to_string();
        // Same length as a real session (32 chars), but not hex.
        let fake_session = "g".repeat(32);
        let owner = format!("{}-m{fake_session}", memory_owner_base(&installation));
        assert!(!owned_by_installation(&owner, &installation));
    }

    #[test]
    fn the_digest_prefix_can_never_be_a_persisted_installation_id() {
        // A digest form the fallback base can actually produce.
        let long_installation = "b".repeat(63);
        let digest = memory_owner_base(&long_installation).into_owned();
        assert!(digest.starts_with(MEMORY_OWNER_DIGEST_PREFIX));
        assert!(
            !is_valid_installation_id(&digest),
            "a raw id equal to another installation's digest must never load, \
             or owned_by_installation could mistake that installation's memory \
             sessions for its own"
        );
    }

    lazy_static! {
        static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn overlapping_initializers_adopt_the_same_identifier() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let ids: Vec<String> = (0..4)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);

                std::thread::spawn(move || {
                    barrier.wait();
                    load_or_create_installation_id().expect("identifier")
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();

        assert!(is_valid_installation_id(&ids[0]), "{}", ids[0]);
        assert!(
            ids.iter().all(|id| *id == ids[0]),
            "concurrent initializers must converge on one identifier: {ids:?}"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("installation_id")).unwrap(),
            ids[0]
        );
    }

    #[test]
    fn a_generated_installation_id_is_the_full_uuid_and_round_trips() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());

        let generated = load_or_create_installation_id().expect("identifier");
        assert_eq!(
            generated.len(),
            32,
            "the full 32-hex-character uuid must be persisted, not a truncated prefix: \
             {generated}"
        );
        assert!(is_valid_installation_id(&generated), "{generated}");

        let reloaded = load_or_create_installation_id().expect("identifier reload");
        assert_eq!(
            reloaded, generated,
            "a second load must read back the exact same identifier"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("installation_id")).unwrap(),
            generated
        );
    }

    #[test]
    fn an_interrupted_write_is_repaired_on_the_next_launch() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
        // What a crash between create and write leaves behind.
        fs::write(dir.path().join("installation_id"), "").unwrap();

        let id = load_or_create_installation_id().expect("an empty file must be replaced");

        assert!(is_valid_installation_id(&id), "{id}");
        assert_eq!(
            fs::read_to_string(dir.path().join("installation_id")).unwrap(),
            id
        );
    }

    #[test]
    fn a_lock_file_left_by_a_dead_process_does_not_block_initialization() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
        // The file a crashed process left behind. Its advisory lock died with
        // it, so the file alone must not block anyone.
        let lock_path = dir.path().join("installation_id.lock");
        fs::write(&lock_path, "").unwrap();

        let id = load_or_create_installation_id().expect("a released lock must not block");

        assert!(is_valid_installation_id(&id), "{id}");
    }

    use crate::test_utils::EnvVarGuard;

    #[test]
    fn test_get_config_dir_kftray_var() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let config_dir = get_config_dir().unwrap();
        assert_eq!(config_dir, PathBuf::from("/custom/config/dir"));
    }

    #[test]
    fn test_get_config_dir_xdg_var() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard1 = EnvVarGuard::remove("KFTRAY_CONFIG");
        let _guard2 = EnvVarGuard::set("XDG_CONFIG_HOME", "/xdg/config/home");
        let config_dir = get_config_dir().unwrap();
        let expected_path = PathBuf::from("/xdg/config/home/kftray");
        assert_eq!(config_dir, expected_path);
    }

    #[test]
    fn test_get_config_dir_default_home() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let fake_home = temp_dir.path().to_str().unwrap();

        let _guard1 = EnvVarGuard::remove("KFTRAY_CONFIG");
        let _guard2 = EnvVarGuard::remove("XDG_CONFIG_HOME");
        let _guard3 = EnvVarGuard::set("HOME", fake_home);

        let config_dir = get_config_dir().unwrap();
        let expected_path = PathBuf::from(fake_home).join(".kftray");
        assert_eq!(config_dir, expected_path);
    }

    #[test]
    fn test_get_config_dir_multiple_fallbacks() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let fake_xdg = temp_dir.path().join("xdg").to_str().unwrap().to_string();
        let fake_home = temp_dir.path().join("home").to_str().unwrap().to_string();

        let _guard1 = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/path");
        let _guard2 = EnvVarGuard::set("XDG_CONFIG_HOME", &fake_xdg);
        let _guard3 = EnvVarGuard::set("HOME", &fake_home);
        assert_eq!(get_config_dir().unwrap(), PathBuf::from("/custom/path"));

        let _guard4 = EnvVarGuard::remove("KFTRAY_CONFIG");
        assert_eq!(
            get_config_dir().unwrap(),
            PathBuf::from(&fake_xdg).join("kftray")
        );
    }

    #[test]
    fn test_get_log_folder_path() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let log_folder_path = get_log_folder_path().unwrap();
        assert_eq!(
            log_folder_path,
            PathBuf::from("/custom/config/dir/http_logs")
        );
    }

    #[test]
    fn test_get_db_file_path() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let db_file_path = get_db_file_path().unwrap();
        assert_eq!(db_file_path, PathBuf::from("/custom/config/dir/configs.db"));
    }

    #[test]
    fn test_get_pod_manifest_path() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let pod_manifest_path = get_pod_manifest_path().unwrap();
        assert_eq!(
            pod_manifest_path,
            PathBuf::from("/custom/config/dir/proxy_manifest.json")
        );
    }

    #[test]
    fn test_get_app_log_path() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let app_log_path = get_app_log_path().unwrap();
        assert_eq!(app_log_path, PathBuf::from("/custom/config/dir/app.log"));
    }

    #[test]
    fn test_get_window_state_path() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = EnvVarGuard::set("KFTRAY_CONFIG", "/custom/config/dir");
        let window_state_path = get_window_state_path().unwrap();
        assert_eq!(
            window_state_path,
            PathBuf::from("/custom/config/dir/window_position.json")
        );
    }

    #[test]
    fn test_get_kubeconfig_paths() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();

        let fake_home_dir = temp_dir.path().join("fake_home");
        fs::create_dir_all(&fake_home_dir).unwrap();
        let _guard_home = EnvVarGuard::set("HOME", fake_home_dir.to_str().unwrap());

        let custom_kubeconfig_path = temp_dir.path().join("custom_kube_config");
        fs::write(&custom_kubeconfig_path, "mock kubeconfig content").unwrap();

        let _guard_kube = EnvVarGuard::set("KUBECONFIG", custom_kubeconfig_path.to_str().unwrap());
        let kubeconfig_paths = get_kubeconfig_paths().unwrap();
        assert_eq!(kubeconfig_paths, vec![custom_kubeconfig_path.clone()]);

        let _guard_kube2 = EnvVarGuard::remove("KUBECONFIG");
        let kube_dir = fake_home_dir.join(".kube");
        fs::create_dir_all(&kube_dir).unwrap();
        let expected_default_path = kube_dir.join("config");
        fs::write(&expected_default_path, "mock kubeconfig content").unwrap();

        let kubeconfig_paths = get_kubeconfig_paths().unwrap();
        assert_eq!(kubeconfig_paths, vec![expected_default_path]);
    }

    #[test]
    fn test_get_kubeconfig_paths_with_multiple_paths() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let config1 = temp_dir.path().join("config1");
        let config2 = temp_dir.path().join("config2");

        fs::write(&config1, "kubeconfig1").unwrap();
        fs::write(&config2, "kubeconfig2").unwrap();

        let separator = if cfg!(windows) { ";" } else { ":" };
        let kubeconfig_env = format!(
            "{}{}{}",
            config1.to_str().unwrap(),
            separator,
            config2.to_str().unwrap()
        );

        let _guard = EnvVarGuard::set("KUBECONFIG", &kubeconfig_env);

        let paths = get_kubeconfig_paths().unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&config1));
        assert!(paths.contains(&config2));
    }

    #[test]
    fn test_get_kubeconfig_paths_error() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let home_path = temp_dir.path().to_str().unwrap();
        let _guard_home = EnvVarGuard::set("HOME", home_path);
        let _guard_kube = EnvVarGuard::remove("KUBECONFIG");

        let kube_dir = temp_dir.path().join(".kube");
        if kube_dir.exists() {
            let config_path = kube_dir.join("config");
            if config_path.exists() {
                std::fs::remove_file(config_path).unwrap();
            }
        }

        let result = get_kubeconfig_paths();
        assert!(
            result.is_err(),
            "get_kubeconfig_paths() should return error when no config exists"
        );

        let error_msg = result.unwrap_err().to_string();
        assert_eq!(error_msg, "Unable to determine kubeconfig path");
    }

    #[test]
    fn test_get_config_dir_using_home_dir_fallback() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_kftray = EnvVarGuard::remove("KFTRAY_CONFIG");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        if let Ok(_config_dir) = get_config_dir() {
            assert!(get_log_folder_path().is_ok());
            assert!(get_db_file_path().is_ok());
            assert!(get_pod_manifest_path().is_ok());
            assert!(get_app_log_path().is_ok());
            assert!(get_window_state_path().is_ok());
        } else {
            assert!(get_config_dir().is_err());
        }
    }
}
