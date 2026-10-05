use std::env;
use std::path::{
    Path,
    PathBuf,
};

use crate::utils::config_dir::{
    DB_FILE_NAME,
    get_config_dir,
};

#[derive(Clone, Debug)]
pub struct ConfigLocation {
    path: PathBuf,
    origin: String,
}

impl ConfigLocation {
    fn new(path: PathBuf, origin: String) -> Self {
        ConfigLocation { path, origin }
    }
}

/// Only a directory holding the config database counts: the helper keeps its
/// socket and address pool under `~/.kftray` regardless of `XDG_CONFIG_HOME`,
/// so a directory with only those files is not a competing configuration.
fn holds_config(path: &Path) -> bool {
    path.join(DB_FILE_NAME).is_file()
}

/// Lists the candidate directories that hold a config database, and the one the
/// app actually uses. The active location comes from `get_config_dir()` rather
/// than from the database filter: an empty `KFTRAY_CONFIG` directory is still
/// where the app reads and writes, even when a lower-priority one has data.
pub fn detect_multiple_configs() -> (Vec<ConfigLocation>, Option<ConfigLocation>) {
    let candidates = [
        (
            "KFTRAY_CONFIG",
            env::var_os("KFTRAY_CONFIG").map(PathBuf::from),
        ),
        (
            "XDG_CONFIG_HOME",
            env::var_os("XDG_CONFIG_HOME").map(|dir| PathBuf::from(dir).join("kftray")),
        ),
        (
            "HOME",
            env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".kftray")),
        ),
    ];
    let active_path = get_config_dir().ok();

    let mut config_locations = Vec::new();
    let mut active_config: Option<ConfigLocation> = None;

    for (origin, path) in candidates {
        let Some(path) = path else { continue };
        let location = ConfigLocation::new(path, origin.into());

        if active_config.is_none() && active_path.as_ref() == Some(&location.path) {
            active_config = Some(location.clone());
        }
        if holds_config(&location.path) {
            config_locations.push(location);
        }
    }

    (config_locations, active_config)
}

pub fn format_alert_message(
    configs: Vec<ConfigLocation>, active_config: Option<ConfigLocation>,
) -> String {
    let msg = configs
        .into_iter()
        .map(|config| format!(" * {}: {}", config.origin, config.path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let active_config_msg = if let Some(active) = active_config {
        format!(
            "Active Configuration:\n * {}: {}\n\n",
            active.origin,
            active.path.display()
        )
    } else {
        "Active Configuration:\n * No active configuration detected.\n\n".to_string()
    };

    format!(
        "Multiple configuration directories have been detected in the following locations:\n\n{}\n\n\
        Environment Variables Checked (in order of precedence):\n\
        - KFTRAY_CONFIG: {}\n\
        - XDG_CONFIG_HOME: {}\n\
        - HOME: {}\n\n\
        {}\
        To resolve this issue, please:\n\
        1. Move or delete the extra configuration directories.\n\
        2. Ensure that the remaining directory is in the correct location.\n\n\
        Recommended Directory:\n\
        * {}\n",
        msg,
        env::var("KFTRAY_CONFIG").unwrap_or_else(|_| "Not set".to_string()),
        env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| "Not set".to_string()),
        env::var("HOME").unwrap_or_else(|_| "Not set".to_string()),
        active_config_msg,
        dirs::home_dir().map_or("<home_directory_not_found>".to_string(), |p| p
            .join(".kftray")
            .display()
            .to_string())
    )
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use lazy_static::lazy_static;
    use parking_lot::Mutex;
    use tempfile::tempdir;

    use super::*;

    lazy_static! {
        static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

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
                Some(val) => unsafe { env::set_var(&key, val) },

                None => unsafe { env::remove_var(&key) },
            }
        }
    }

    fn setup_env_and_dirs(
        kftray: Option<&str>, xdg: Option<&str>, home: Option<&str>,
    ) -> (tempfile::TempDir, Vec<(String, Option<String>)>) {
        let preserved = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);
        let temp_dir = tempdir().unwrap();
        let base_path = temp_dir.path();

        if let Some(dir) = kftray {
            let path = base_path.join(dir);
            create_config(&path);
            unsafe { env::set_var("KFTRAY_CONFIG", path) };
        } else {
            unsafe { env::remove_var("KFTRAY_CONFIG") };
        }

        if let Some(dir) = xdg {
            let path = base_path.join(dir);
            create_config(&path.join("kftray"));
            unsafe { env::set_var("XDG_CONFIG_HOME", path) };
        } else {
            unsafe { env::remove_var("XDG_CONFIG_HOME") };
        }

        if let Some(dir) = home {
            let path = base_path.join(dir);
            create_config(&path.join(".kftray"));
            unsafe { env::set_var("HOME", path) };
        } else if !preserved.iter().any(|(k, v)| k == "HOME" && v.is_some()) {
            unsafe { env::remove_var("HOME") };
        }

        (temp_dir, preserved)
    }

    fn create_config(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(DB_FILE_NAME), b"").unwrap();
    }

    struct StrictEnvGuard {
        saved_vars: Vec<(String, Option<String>)>,
    }

    impl StrictEnvGuard {
        fn new(keys: &[&str]) -> Self {
            let saved_vars = keys
                .iter()
                .map(|&key| (key.to_string(), env::var(key).ok()))
                .collect::<Vec<_>>();

            for key in keys {
                unsafe { env::remove_var(key) };
            }

            StrictEnvGuard { saved_vars }
        }
    }

    impl Drop for StrictEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved_vars.drain(..) {
                match value {
                    Some(val) => unsafe { env::set_var(key, val) },

                    None => unsafe { env::remove_var(key) },
                }
            }
        }
    }

    #[test]
    fn test_detect_multiple_configs_none() {
        struct IsolatedEnvGuard {
            saved_vars: Vec<(String, Option<String>)>,
            _temp_dir: tempfile::TempDir,
        }

        impl IsolatedEnvGuard {
            fn new() -> Self {
                let vars = ["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]
                    .iter()
                    .map(|&k| (k.to_string(), env::var(k).ok()))
                    .collect();

                for var in &["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"] {
                    unsafe { env::remove_var(var) };
                }

                let temp_dir = tempfile::tempdir().unwrap();

                IsolatedEnvGuard {
                    saved_vars: vars,
                    _temp_dir: temp_dir,
                }
            }
        }

        impl Drop for IsolatedEnvGuard {
            fn drop(&mut self) {
                for (key, value) in self.saved_vars.drain(..) {
                    match value {
                        // code.
                        Some(val) => unsafe { env::set_var(&key, val) },

                        // code.
                        None => unsafe { env::remove_var(&key) },
                    }
                }
            }
        }

        let _env_guard = IsolatedEnvGuard::new();

        unsafe { env::remove_var("KFTRAY_CONFIG") };

        unsafe { env::remove_var("XDG_CONFIG_HOME") };

        unsafe { env::remove_var("HOME") };

        let (configs, active) = detect_multiple_configs();
        assert!(
            configs.is_empty(),
            "Expected no config locations, got: {:?}",
            configs
                .iter()
                .map(|c| format!("{}: {}", c.origin, c.path.display()))
                .collect::<Vec<_>>()
        );
        assert!(active.is_none(), "Expected no active config");
    }

    #[test]
    fn test_detect_multiple_configs_kftray_only() {
        let _lock = ENV_TEST_MUTEX.lock();
        let _env_guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempfile::tempdir().unwrap();
        let kftray_dir = temp_dir.path().join("kftray_only_test");
        create_config(&kftray_dir);

        let home_temp_dir = tempfile::tempdir().unwrap();

        unsafe { env::set_var("KFTRAY_CONFIG", kftray_dir.to_str().unwrap()) };

        unsafe { env::set_var("HOME", home_temp_dir.path().to_str().unwrap()) };

        assert!(kftray_dir.exists());
        assert!(env::var("KFTRAY_CONFIG").is_ok());
        assert!(env::var("HOME").is_ok());
        assert!(env::var("XDG_CONFIG_HOME").is_err());

        let (configs, active) = detect_multiple_configs();

        assert_eq!(
            configs.len(),
            1,
            "Expected exactly one config from KFTRAY_CONFIG, got: {configs:?}"
        );

        assert_eq!(configs[0].origin, "KFTRAY_CONFIG");
        assert!(active.is_some());
        assert_eq!(active.unwrap().origin, "KFTRAY_CONFIG");
    }

    #[test]
    fn test_detect_multiple_configs_xdg_only() {
        let _lock = ENV_TEST_MUTEX.lock();
        let _env_guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let xdg_temp_dir = tempfile::tempdir().unwrap();
        let home_temp_dir = tempfile::tempdir().unwrap();

        let xdg_path = xdg_temp_dir.path();
        let kftray_dir = xdg_path.join("kftray");
        create_config(&kftray_dir);

        unsafe { env::set_var("XDG_CONFIG_HOME", xdg_path.to_str().unwrap()) };

        unsafe { env::set_var("HOME", home_temp_dir.path().to_str().unwrap()) };

        unsafe { env::remove_var("KFTRAY_CONFIG") };

        assert!(kftray_dir.exists(), "XDG kftray dir should exist");
        assert!(
            env::var("XDG_CONFIG_HOME").is_ok(),
            "XDG_CONFIG_HOME should be set"
        );
        assert!(env::var("HOME").is_ok(), "HOME should be set");
        assert!(
            env::var("KFTRAY_CONFIG").is_err(),
            "KFTRAY_CONFIG should not be set"
        );

        let (configs, active) = detect_multiple_configs();
        assert_eq!(
            configs.len(),
            1,
            "Expected exactly one config from XDG_CONFIG_HOME, got: {:?}",
            configs
                .iter()
                .map(|c| format!("{}: {}", c.origin, c.path.display()))
                .collect::<Vec<_>>()
        );

        if !configs.is_empty() {
            assert_eq!(
                configs[0].origin, "XDG_CONFIG_HOME",
                "Config origin should be XDG_CONFIG_HOME, got: {}",
                configs[0].origin
            );
            assert!(active.is_some(), "Should have an active config");
            let active_config = active.unwrap();
            assert_eq!(
                active_config.origin, "XDG_CONFIG_HOME",
                "Active config origin should be XDG_CONFIG_HOME, got: {}",
                active_config.origin
            );
        }
    }

    #[test]
    fn test_detect_multiple_configs_home_only() {
        let _lock = ENV_TEST_MUTEX.lock();
        let (_temp_dir, preserved_vars) = setup_env_and_dirs(None, None, Some("home_dir"));

        unsafe { env::remove_var("KFTRAY_CONFIG") };

        unsafe { env::remove_var("XDG_CONFIG_HOME") };

        let (configs, active) = detect_multiple_configs();

        assert_eq!(
            configs.len(),
            1,
            "Expected exactly one config location (HOME), got: {:?}",
            configs.iter().map(|c| &c.origin).collect::<Vec<_>>()
        );
        assert_eq!(configs[0].origin, "HOME");
        assert!(active.is_some());
        assert_eq!(active.unwrap().origin, "HOME");

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_detect_multiple_configs_ignores_helper_only_home_dir() {
        let _lock = ENV_TEST_MUTEX.lock();
        let (temp_dir, preserved_vars) = setup_env_and_dirs(None, Some("xdg_dir"), None);

        let home_path = temp_dir.path().join("home_dir");
        let helper_dir = home_path.join(".kftray");
        fs::create_dir_all(&helper_dir).unwrap();
        fs::write(helper_dir.join("kftray-helper.sock"), b"").unwrap();
        unsafe { env::set_var("HOME", &home_path) };

        let (configs, active) = detect_multiple_configs();

        restore_env_vars(preserved_vars);
        assert_eq!(
            configs
                .iter()
                .map(|c| c.origin.as_str())
                .collect::<Vec<_>>(),
            ["XDG_CONFIG_HOME"]
        );
        assert_eq!(active.unwrap().origin, "XDG_CONFIG_HOME");
    }

    #[test]
    fn test_detect_multiple_configs_active_follows_empty_kftray_config() {
        let _lock = ENV_TEST_MUTEX.lock();
        let (temp_dir, preserved_vars) =
            setup_env_and_dirs(None, Some("xdg_dir"), Some("home_dir"));

        let kftray_dir = temp_dir.path().join("kftray_empty");
        fs::create_dir_all(&kftray_dir).unwrap();
        unsafe { env::set_var("KFTRAY_CONFIG", &kftray_dir) };

        let (configs, active) = detect_multiple_configs();

        restore_env_vars(preserved_vars);
        assert_eq!(
            configs
                .iter()
                .map(|c| c.origin.as_str())
                .collect::<Vec<_>>(),
            ["XDG_CONFIG_HOME", "HOME"]
        );
        let active = active.unwrap();
        assert_eq!(active.origin, "KFTRAY_CONFIG");
        assert_eq!(active.path, kftray_dir);
    }

    #[test]
    fn test_format_alert_message_multiple() {
        let _lock = ENV_TEST_MUTEX.lock();
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let kftray_path = "/custom/kftray/path";
        let xdg_base_path = "/custom/xdg/path";
        let home_base_path = "/custom/home/path";

        unsafe { env::set_var("KFTRAY_CONFIG", kftray_path) };

        unsafe { env::set_var("XDG_CONFIG_HOME", xdg_base_path) };

        unsafe { env::set_var("HOME", home_base_path) };

        let xdg_path = PathBuf::from(&xdg_base_path).join("kftray");
        let home_path = PathBuf::from(&home_base_path).join(".kftray");

        let configs = vec![
            ConfigLocation::new(PathBuf::from(kftray_path), "KFTRAY_CONFIG".into()),
            ConfigLocation::new(xdg_path.clone(), "XDG_CONFIG_HOME".into()),
            ConfigLocation::new(home_path.clone(), "HOME".into()),
        ];
        let active = configs[0].clone();

        let message = format_alert_message(configs, Some(active));
        println!("Message content: {message}");

        assert!(message.contains("Multiple configuration directories have been detected"));
        assert!(message.contains(&format!("KFTRAY_CONFIG: {kftray_path}")));
        assert!(message.contains(&format!("XDG_CONFIG_HOME: {}", xdg_path.display())));
        assert!(message.contains(&format!("HOME: {}", home_path.display())));

        assert!(message.contains("Active Configuration"));
        assert!(message.contains("Environment Variables"));
        assert!(message.contains("KFTRAY_CONFIG:"));
        assert!(message.contains("XDG_CONFIG_HOME:"));
        assert!(message.contains("HOME:"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_format_alert_message_no_active() {
        let _lock = ENV_TEST_MUTEX.lock();
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let configs = vec![
            ConfigLocation::new(PathBuf::from("/path/to/kftray"), "KFTRAY_CONFIG".into()),
            ConfigLocation::new(
                PathBuf::from("/path/to/xdg/kftray"),
                "XDG_CONFIG_HOME".into(),
            ),
        ];

        let message = format_alert_message(configs, None);
        println!("Message content: {message}");

        assert!(message.contains("Multiple configuration directories have been detected"));
        assert!(message.contains("No active configuration detected"));

        restore_env_vars(preserved_vars);
    }

    #[test]
    fn test_alert_multiple_configs_show_dialog() {
        let configs = vec![
            ConfigLocation::new(PathBuf::from("/path/to/config1"), "KFTRAY_CONFIG".into()),
            ConfigLocation::new(PathBuf::from("/path/to/config2"), "HOME".into()),
        ];

        let active = Some(configs[0].clone());

        let message = format_alert_message(configs, active);

        assert!(!message.is_empty(), "Alert message should not be empty");
        assert!(
            message.contains("Multiple configuration directories"),
            "Message should mention multiple configs"
        );
        assert!(
            message.contains("KFTRAY_CONFIG: /path/to/config1"),
            "Message should contain the first config path"
        );
        assert!(
            message.contains("HOME: /path/to/config2"),
            "Message should contain the second config path"
        );
        assert!(
            message.contains("Active Configuration"),
            "Message should mention active configuration"
        );
    }

    #[test]
    fn test_detect_multiple_configs_with_non_existent_paths() {
        let _lock = ENV_TEST_MUTEX.lock();

        let _guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempfile::tempdir().unwrap();
        let non_existent_path = temp_dir.path().join("non_existent");

        let (configs_before, _) = detect_multiple_configs();
        assert_eq!(configs_before.len(), 0, "Should have no configs initially");

        if let Some(path_str) = non_existent_path.to_str() {
            unsafe { env::set_var("KFTRAY_CONFIG", path_str) };
        }

        let (configs_after, _active) = detect_multiple_configs();
        assert_eq!(
            configs_after.len(),
            0,
            "No configs should be found with non-existent paths"
        );
    }

    #[test]
    fn test_format_alert_message_with_fake_paths() {
        let _lock = ENV_TEST_MUTEX.lock();
        let preserved_vars = preserve_env_vars(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        unsafe { env::remove_var("KFTRAY_CONFIG") };

        unsafe { env::remove_var("XDG_CONFIG_HOME") };

        let configs = vec![
            ConfigLocation::new(PathBuf::from("/fake/path1"), "Origin1".into()),
            ConfigLocation::new(PathBuf::from("/fake/path2"), "Origin2".into()),
        ];

        let message = format_alert_message(configs, None);
        println!("Message content: {message}");

        assert!(message.contains("Multiple configuration directories have been detected"));
        assert!(message.contains("Origin1: /fake/path1"));
        assert!(message.contains("Origin2: /fake/path2"));
        assert!(message.contains("No active configuration"));

        assert!(message.contains("Environment Variables"));

        restore_env_vars(preserved_vars);
    }
}
