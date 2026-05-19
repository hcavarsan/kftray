#![allow(clippy::needless_return)]

pub mod cli;
pub mod core;
pub mod logging;
pub mod stdin;
pub mod tui;
#[cfg(not(debug_assertions))]
pub mod updater;
pub mod utils;

#[cfg(test)]
mod tests;

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
    StdoutLoggerInitializer,
    TuiLoggerInitializer,
};

/// Entry point for the kftui binary. Parses CLI args, wires up the logger
/// pipeline for the chosen run mode, and hands control to `CliHandler`.
///
/// Exposed from the library so `main.rs` can stay a thin shim and the
/// integration test suite can drive the same code path the binary uses.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    kftray_ssl::install_default_keyring_store();
    kftray_ssl::ensure_crypto_provider_installed();

    let cli = Cli::parse();

    let mut config = if let Some(level_str) = cli.log_level.as_ref() {
        LogConfig::new(logging::parse_level(level_str))
    } else {
        LogConfig::with_default_level(log::LevelFilter::Warn)
    };

    // The tui-logger crate spawns a background hot-buffer drain thread that
    // burns CPU even when no TUI is being rendered, preempting tokio
    // workers handling port-forward traffic. Use it only when the TUI is
    // actually shown.
    //
    // - --logs-to-file: explicit file output (env_logger -> file pipe)
    // - --non-interactive: stdout (env_logger -> stdout, honors RUST_LOG)
    // - default (TUI): in-memory ring buffer rendered by the TUI
    let initializer: Box<dyn LoggerInitializer> = if cli.logs_to_file {
        let log_path = get_app_log_path()?;
        config = config.with_file_output(log_path);
        Box::new(FileLoggerInitializer)
    } else if cli.non_interactive {
        Box::new(StdoutLoggerInitializer)
    } else {
        Box::new(TuiLoggerInitializer)
    };

    initializer.initialize(&config)?;

    let logger_state = LoggerState::new(config);
    let handler = CliHandler::new(cli, logger_state);
    handler.run().await
}
