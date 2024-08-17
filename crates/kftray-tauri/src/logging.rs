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
