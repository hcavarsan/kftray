mod config;
mod initializer;
mod state;

pub(crate) use config::LogConfig;
pub(crate) use initializer::{
    FileLoggerInitializer,
    LoggerInitializer,
    StdoutLoggerInitializer,
    TuiLoggerInitializer,
};
pub(crate) use state::LoggerState;

pub(crate) fn parse_level(level: &str) -> log::LevelFilter {
    match level.to_lowercase().as_str() {
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        "off" => log::LevelFilter::Off,
        _ => log::LevelFilter::Info,
    }
}
