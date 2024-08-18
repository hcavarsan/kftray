use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    Arc,
    Mutex,
};

use kftray_commons::utils::config_dir::get_app_log_path;
use log::{
    Metadata,
    Record,
};
use once_cell::sync::Lazy;

pub struct AppLogger {
    pub buffer: Arc<Mutex<String>>,
    pub file: Option<Arc<Mutex<std::fs::File>>>,
}

impl AppLogger {
    fn new(buffer: Arc<Mutex<String>>, file: Option<Arc<Mutex<std::fs::File>>>) -> Self {
        AppLogger { buffer, file }
    }
}

impl log::Log for AppLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::Level::Info && metadata.target().starts_with("kftray_portforward")
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let log_entry = format!("{}\n", record.args());

            {
                let mut buffer = self.buffer.lock().unwrap();
                buffer.push_str(&log_entry);
            }

            if let Some(file) = &self.file {
                let mut file = file.lock().unwrap();
                if let Err(_e) = file.write_all(log_entry.as_bytes()) {}
            }
        }
    }

    fn flush(&self) {}
}

pub static LOGGER: Lazy<AppLogger> = Lazy::new(|| {
    let buffer = Arc::new(Mutex::new(String::new()));
    let file = if std::env::var("KFTUI_LOGS").unwrap_or_default() == "enabled" {
        get_app_log_path().ok().and_then(|path| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(|file| Arc::new(Mutex::new(file)))
        })
    } else {
        None
    };
    AppLogger::new(buffer, file)
});

pub fn init_logger() -> Result<(), Box<dyn std::error::Error>> {
    Ok(log::set_logger(&*LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info))?)
}
