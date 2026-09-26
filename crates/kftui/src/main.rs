#![allow(clippy::needless_return)]
mod cli;
mod core;
mod logging;
mod stdin;
mod tui;
mod updater;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = runtime.block_on(run());

    // Gives a task a bounded shutdown drain had to detach (a start or stop
    // that outran its own budget) a further bounded chance to finish its own
    // rollback or cleanup before the runtime, and everything still running
    // on it, is torn down. Tokio drops whatever is still running once this
    // returns, rather than awaiting it further.
    runtime.shutdown_timeout(crate::core::port_forward::CLEANUP_RECONCILE_TIMEOUT);

    result
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    kftray_portforward::ssl::install_default_keyring_store();
    kftray_portforward::ssl::ensure_crypto_provider_installed();

    // Resolves and applies the PATH env fixup once, here, before anything
    // else on the runtime is running: `create_client_with_specific_context`
    // does the same lazily via `unsafe` env mutation on first use, which is
    // racy against any other task already reading the environment once the
    // runtime has other work in flight.
    kftray_portforward::warm_up_path_env().await;

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
