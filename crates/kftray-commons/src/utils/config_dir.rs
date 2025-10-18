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
