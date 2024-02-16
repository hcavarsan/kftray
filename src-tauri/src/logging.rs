use std::{env, fs::OpenOptions, path::PathBuf};

pub fn get_log_path() -> PathBuf {
    let home_dir = dirs::home_dir().expect("Could not find the home directory");
    home_dir.join(".kftray").join("app.log")
}

pub fn setup_logging() {
    let log_filter = match env::var("RUST_LOG") {
        Ok(filter) => filter.parse().unwrap_or(log::LevelFilter::Info),
        Err(_) => log::LevelFilter::Off,
    };

    if env::var("KFTRAY_DEBUG").is_ok() {
        let log_path = get_log_path();
        let log_dir = log_path.parent().expect("Could not find the log directory");
        std::fs::create_dir_all(log_dir).expect("Could not create log directory");

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("Could not open log file");

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
}
