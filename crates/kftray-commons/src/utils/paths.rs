use std::env;
use std::path::PathBuf;

use crate::error::{
    Error,
    Result,
};

pub async fn get_config_dir() -> Result<PathBuf> {
    if let Ok(config_dir) = env::var("KFTRAY_CONFIG") {
        return Ok(PathBuf::from(config_dir));
    }

    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        let mut path = PathBuf::from(config_home);
        path.push("kftray");
        return Ok(path);
    }

    dirs::home_dir()
        .map(|mut path| {
            path.push(".kftray");
            path
        })
        .ok_or_else(|| Error::config("Unable to determine configuration directory"))
}

pub async fn ensure_config_dir() -> Result<PathBuf> {
    let path = get_config_dir().await?;
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub async fn get_db_path() -> Result<PathBuf> {
    let mut path = get_config_dir().await?;
    path.push("configs.db");
    Ok(path)
}

pub async fn get_log_dir() -> Result<PathBuf> {
    let mut path = get_config_dir().await?;
    path.push("logs");
    Ok(path)
}

pub async fn get_app_log_path() -> Result<PathBuf> {
    let mut path = get_config_dir().await?;
    path.push("app.log");
    Ok(path)
}

pub async fn get_window_state_path() -> Result<PathBuf> {
    let mut path = get_config_dir().await?;
    path.push("window_state.json");
    Ok(path)
}

pub async fn get_pod_manifest_path() -> Result<PathBuf> {
    let mut config_path = get_config_dir().await?;
    config_path.push("proxy_manifest.json");
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
        Err(Error::config("Unable to determine kubeconfig path"))
    } else {
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn test_path_resolution() {
        let temp_dir = tempdir().unwrap();
        env::set_var("PORT_FORWARD_CONFIG", temp_dir.path());

        let config_dir = get_config_dir().await.unwrap();
        assert_eq!(config_dir, temp_dir.path());

        let db_path = get_db_path().await.unwrap();
        assert!(db_path.ends_with("configs.db"));
    }
}
