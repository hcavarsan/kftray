#![allow(clippy::needless_return)]
mod cli;
mod core;
mod logging;
mod stdin;
mod tui;
mod utils;

use clap::Parser;
use kftray_commons::utils::config_dir::get_app_log_path;

use crate::cli::{
    Cli,
    CliHandler,
};
use crate::logging::{
    FileLoggerInitializer,
    LogConfig,
    LoggerInitializer,
    LoggerState,
    TuiLoggerInitializer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let mut config = if let Some(level_str) = cli.log_level.as_ref() {
        LogConfig::new(logging::parse_level(level_str))
    } else {
        LogConfig::with_default_level(log::LevelFilter::Warn)
    };

    let initializer: Box<dyn LoggerInitializer>;

    if cli.logs_to_file {
        let log_path = get_app_log_path()?;
        config = config.with_file_output(log_path);
        initializer = Box::new(FileLoggerInitializer);
    } else {
        initializer = Box::new(TuiLoggerInitializer);
    }

    initializer.initialize(&config)?;

    let logger_state = LoggerState::new(config);
    let handler = CliHandler::new(cli, logger_state);
    handler.run().await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };

    static TUI_LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn test_initialize_logger() {
        if !TUI_LOGGER_INITIALIZED.load(Ordering::SeqCst) {
            let result = tui_logger::init_logger(log::LevelFilter::Debug);
            TUI_LOGGER_INITIALIZED.store(result.is_ok(), Ordering::SeqCst);
        }

        tui_logger::set_default_level(log::LevelFilter::Debug);
    }
}
