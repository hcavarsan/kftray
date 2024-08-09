use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{
    Arc,
    Mutex,
};

use log::{
    Metadata,
    Record,
};
use once_cell::sync::Lazy;

pub struct AppLogger {
    pub buffer: Arc<Mutex<String>>,
    pub file: Arc<Mutex<std::fs::File>>,
}

impl AppLogger {
    fn new(buffer: Arc<Mutex<String>>, file: Arc<Mutex<std::fs::File>>) -> Self {
        AppLogger { buffer, file }
    }
}

impl log::Log for AppLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let target = record.target();
            if target.starts_with("sqlx") {
                return;
            }

            let log_entry = format!("{}\n", record.args());

            {
                let mut buffer = self.buffer.lock().unwrap();
                buffer.push_str(&log_entry);
            }

            {
                let mut file = self.file.lock().unwrap();
                if let Err(e) = file.write_all(log_entry.as_bytes()) {
                    eprintln!("Failed to write log to file: {}", e);
                }
            }
        }
    }

    fn flush(&self) {}
}

pub static LOGGER: Lazy<AppLogger> = Lazy::new(|| {
    let buffer = Arc::new(Mutex::new(String::new()));
    let file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open("app.log")
            .unwrap(),
    ));
    AppLogger::new(buffer, file)
});
