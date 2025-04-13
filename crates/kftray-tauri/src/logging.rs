use std::{
    env,
    fs::OpenOptions,
    io,
};

use kftray_commons::utils::config_dir::get_app_log_path;

pub fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    let log_filter = match env::var("RUST_LOG") {
        Ok(filter) => filter.parse().unwrap_or(log::LevelFilter::Info),
        Err(_) => log::LevelFilter::Off,
    };

    if env::var("KFTRAY_DEBUG").is_ok() {
        let log_path = get_app_log_path().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let log_dir = log_path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Could not find the log directory")
        })?;

        std::fs::create_dir_all(log_dir)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Could not create log directory"))?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Could not open log file"))?;

        env_logger::Builder::from_default_env()
            .filter_level(log_filter)
            .format_timestamp_secs()
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .init();
    } else {
        env_logger::Builder::new()
            .filter_level(log_filter)
            .format_timestamp_secs()
            .init();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn test_log_level_parsing() {
        let test_cases = vec![
            ("debug", log::LevelFilter::Debug),
            ("info", log::LevelFilter::Info),
            ("warn", log::LevelFilter::Warn),
            ("error", log::LevelFilter::Error),
            ("trace", log::LevelFilter::Trace),
            ("invalid_level", log::LevelFilter::Info), // Should default to Info
        ];

        for (input, expected) in test_cases {
            let result = input.parse().unwrap_or(log::LevelFilter::Info);
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_debug_env_detection() {
        let original_debug = std::env::var("KFTRAY_DEBUG").ok();

        std::env::set_var("KFTRAY_DEBUG", "1");
        assert!(std::env::var("KFTRAY_DEBUG").is_ok());

        std::env::remove_var("KFTRAY_DEBUG");
        assert!(std::env::var("KFTRAY_DEBUG").is_err());

        if let Some(val) = original_debug {
            std::env::set_var("KFTRAY_DEBUG", val);
        }
    }

    #[test]
    fn test_rust_log_env_var() {
        let original_rust_log = env::var("RUST_LOG").ok();

        env::set_var("RUST_LOG", "debug");
        let filter = match env::var("RUST_LOG") {
            Ok(filter) => filter.parse().unwrap_or(log::LevelFilter::Info),
            Err(_) => log::LevelFilter::Off,
        };
        assert_eq!(filter, log::LevelFilter::Debug);

        env::set_var("RUST_LOG", "not_a_valid_level");
        let filter = match env::var("RUST_LOG") {
            Ok(filter) => filter.parse().unwrap_or(log::LevelFilter::Info),
            Err(_) => log::LevelFilter::Off,
        };
        assert_eq!(filter, log::LevelFilter::Info);

        env::remove_var("RUST_LOG");
        let filter = match env::var("RUST_LOG") {
            Ok(filter) => filter.parse().unwrap_or(log::LevelFilter::Info),
            Err(_) => log::LevelFilter::Off,
        };
        assert_eq!(filter, log::LevelFilter::Off);

        if let Some(val) = original_rust_log {
            env::set_var("RUST_LOG", val);
        }
    }

    #[test]
    fn test_log_directory_creation() {
        let temp_dir = tempdir().unwrap();
        let log_dir = temp_dir.path().join("logs");
        let log_path = log_dir.join("app.log");

        if log_dir.exists() {
            fs::remove_dir_all(&log_dir).unwrap();
        }

        assert!(!log_dir.exists());

        fs::create_dir_all(&log_dir).unwrap();

        assert!(log_dir.exists());

        let parent = log_path.parent();
        assert!(parent.is_some());
        assert_eq!(parent.unwrap(), log_dir);
    }
}
