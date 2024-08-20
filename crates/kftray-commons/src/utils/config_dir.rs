use std::{
    env,
    path::PathBuf,
};

use anyhow::Result;

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

    if paths.is_empty() {
        if let Some(mut config_path) = dirs::home_dir() {
            config_path.push(".kube/config");
            if config_path.exists() {
                paths.push(config_path);
            }
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

    use tempfile::TempDir;

    use super::*;

    fn preserve_env_vars(keys: &[&str]) -> Vec<(String, Option<String>)> {
        keys.iter()
            .map(|&key| {
                let value = env::var(key).ok();
                (key.to_string(), value)
            })
            .collect()
    }

    fn restore_env_vars(vars: Vec<(String, Option<String>)>) {
        for (key, value) in vars {
            match value {
                Some(val) => env::set_var(&key, val),
                None => env::remove_var(&key),
            }
        }
    }

    #[test]
    fn test_get_config_dir_kftray_var() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let config_dir = get_config_dir().unwrap();
        assert_eq!(config_dir, PathBuf::from("/custom/config/dir"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_config_dir_xdg_var() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME"]);

        env::remove_var("KFTRAY_CONFIG");
        env::set_var("XDG_CONFIG_HOME", "/xdg/config/home");
        let config_dir = get_config_dir().unwrap();
        assert_eq!(config_dir, PathBuf::from("/xdg/config/home/kftray"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_config_dir_default_home() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME"]);

        env::remove_var("KFTRAY_CONFIG");
        env::remove_var("XDG_CONFIG_HOME");
        let home_dir = dirs::home_dir().unwrap();
        let config_dir = get_config_dir().unwrap();
        assert_eq!(config_dir, home_dir.join(".kftray"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_log_folder_path() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let log_folder_path = get_log_folder_path().unwrap();
        assert_eq!(
            log_folder_path,
            PathBuf::from("/custom/config/dir/http_logs")
        );

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_db_file_path() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let db_file_path = get_db_file_path().unwrap();
        assert_eq!(db_file_path, PathBuf::from("/custom/config/dir/configs.db"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_pod_manifest_path() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let pod_manifest_path = get_pod_manifest_path().unwrap();
        assert_eq!(
            pod_manifest_path,
            PathBuf::from("/custom/config/dir/proxy_manifest.json")
        );

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_app_log_path() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let app_log_path = get_app_log_path().unwrap();
        assert_eq!(app_log_path, PathBuf::from("/custom/config/dir/app.log"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_window_state_path() {
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG"]);

        env::set_var("KFTRAY_CONFIG", "/custom/config/dir");
        let window_state_path = get_window_state_path().unwrap();
        assert_eq!(
            window_state_path,
            PathBuf::from("/custom/config/dir/window_position.json")
        );

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_get_kubeconfig_paths() {
        let preserved_vars = preserve_env_vars(&["KUBECONFIG"]);

        let temp_dir = TempDir::new().unwrap();
        let custom_kubeconfig_path = temp_dir.path().join("custom_kube_config");
        fs::write(&custom_kubeconfig_path, "mock kubeconfig content").unwrap();

        env::set_var("KUBECONFIG", custom_kubeconfig_path.to_str().unwrap());
        let kubeconfig_paths = get_kubeconfig_paths().unwrap();
        assert_eq!(kubeconfig_paths, vec![custom_kubeconfig_path.clone()]);

        env::remove_var("KUBECONFIG");
        let expected_default_path = dirs::home_dir().unwrap().join(".kube/config");
        fs::write(&expected_default_path, "mock kubeconfig content").unwrap();
        let kubeconfig_paths = get_kubeconfig_paths().unwrap();
        assert_eq!(kubeconfig_paths, vec![expected_default_path.clone()]);

        restore_env_vars(preserved_vars);

        fs::remove_file(custom_kubeconfig_path).unwrap();
        fs::remove_file(expected_default_path).unwrap();
    }
}
