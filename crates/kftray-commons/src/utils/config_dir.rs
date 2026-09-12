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
pub fn get_installation_id() -> Result<&'static str, String> {
    static INSTALLATION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    if let Some(id) = INSTALLATION_ID.get() {
        return Ok(id);
    }
    let id = load_or_create_installation_id()?;

    Ok(INSTALLATION_ID.get_or_init(|| id))
}

fn load_or_create_installation_id() -> Result<String, String> {
    let config_dir = get_config_dir()?;
    let path = config_dir.join("installation_id");
    match fs::read_to_string(&path) {
        Ok(stored) => {
            let stored = stored.trim();
            if is_valid_installation_id(stored) {
                return Ok(stored.to_owned());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Failed to read the installation identifier at {}: {error}",
                path.display()
            ));
        }
    }

    fs::create_dir_all(&config_dir).map_err(|error| {
        format!(
            "Failed to create the configuration directory {}: {error}",
            config_dir.display()
        )
    })?;

    // Creation and repair run under a lock file so concurrent initializers
    // adopt one identifier instead of each caching its own. It relies only on
    // exclusive create and rename, which work on filesystems without hard link
    // support such as exFAT.
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
            return Ok(stored);
        }
        let generated = uuid::Uuid::new_v4().simple().to_string()[..12].to_owned();
        publish_installation_id(&config_dir, &path, &generated)?;
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
    const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
    const POLL: std::time::Duration = std::time::Duration::from_millis(25);

    let lock_path = config_dir.join("installation_id.lock");
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| {
            format!(
                "Failed to open the installation identifier lock at {}: {error}",
                lock_path.display()
            )
        })?;

    let deadline = std::time::Instant::now() + LOCK_WAIT;
    loop {
        if try_lock_exclusive(&lock) {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting for the installation identifier lock at {}",
                lock_path.display()
            ));
        }
        std::thread::sleep(POLL);
    }

    let result = write();
    unlock(&lock);
    result
}

#[cfg(unix)]
fn try_lock_exclusive(file: &fs::File) -> bool {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor is owned by `file` and outlives this call.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(unix)]
fn unlock(file: &fs::File) {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor is owned by `file` and outlives this call.
    unsafe {
        libc::flock(file.as_raw_fd(), libc::LOCK_UN);
    }
}

#[cfg(windows)]
fn try_lock_exclusive(file: &fs::File) -> bool {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY,
        LockFileEx,
    };
    use windows::Win32::System::IO::OVERLAPPED;

    let mut overlapped = OVERLAPPED::default();

    // SAFETY: the handle is owned by `file` and outlives this call.
    unsafe {
        LockFileEx(
            HANDLE(file.as_raw_handle()),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
        .is_ok()
    }
}

#[cfg(windows)]
fn unlock(file: &fs::File) {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::UnlockFileEx;
    use windows::Win32::System::IO::OVERLAPPED;

    let mut overlapped = OVERLAPPED::default();

    // SAFETY: the handle is owned by `file` and outlives this call.
    unsafe {
        let _ = UnlockFileEx(
            HANDLE(file.as_raw_handle()),
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        );
    }
}

/// Writes the identifier in full to a temporary file and moves it into place,
/// so the published path is never visible empty.
fn publish_installation_id(
    config_dir: &std::path::Path, path: &std::path::Path, id: &str,
) -> Result<(), String> {
    use std::io::Write;

    let temporary = config_dir.join(format!("installation_id.{}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(id.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(|error| {
        format!(
            "Failed to persist the installation identifier at {}: {error}",
            path.display()
        )
    })
}

fn is_valid_installation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
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
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use lazy_static::lazy_static;
    use tempfile::TempDir;

    use super::*;

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

    struct EnvVarGuard {
        key: String,
        original_value: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let key = key.to_string();
            let original_value = env::var(&key).ok();
            unsafe { env::set_var(&key, value) };
            EnvVarGuard {
                key,
                original_value,
            }
        }

        fn remove(key: &str) -> Self {
            let key = key.to_string();
            let original_value = env::var(&key).ok();
            unsafe { env::remove_var(&key) };
            EnvVarGuard {
                key,
                original_value,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(val) => unsafe { env::set_var(&self.key, val) },

                None => unsafe { env::remove_var(&self.key) },
            }
        }
    }

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
