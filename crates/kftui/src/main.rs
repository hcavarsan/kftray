#![allow(clippy::needless_return)]
mod cli;
mod core;
mod tui;
mod utils;

use std::fs;
use std::process;

use clap::Parser;
use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
use kftray_commons::utils::config::{
    import_configs_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db::init;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{
    GitHubConfig,
    GitHubRepository,
};
use kftray_commons::utils::migration::migrate_configs;
use kftray_portforward::kube::stop_all_port_forward;
use log::error;

use crate::cli::Cli;
use crate::tui::run_tui;

async fn import_configs_from_source(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
    if cli.flush && mode == DatabaseMode::File {
        if let Err(e) = kftray_commons::utils::github::clear_existing_configs_with_mode(mode).await
        {
            error!("Failed to clear existing configs: {e}");
            return Err(format!("Failed to clear existing configs: {e}"));
        }

        if let Err(e) = stop_all_port_forward().await {
            error!("Failed to stop all port forwards: {e:?}");
        }
    }

    if cli.is_github_import() {
        let github_url = cli.get_github_url().unwrap();
        let config_path = cli.get_configs_path_with_default();

        let github_token = std::env::var("GITHUB_TOKEN").ok();

        let github_config = GitHubConfig {
            repo_url: github_url.to_string(),
            config_path,
            use_system_credentials: true,
            github_token,
            flush_existing: false,
        };

        GitHubRepository::import_configs(github_config, mode).await
    } else if let Some(config_path) = cli.get_config_path() {
        let json_content = fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file: {e}"))?;

        import_configs_with_mode(json_content, mode).await
    } else {
        Err("No config source specified".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tui_logger::init_logger(log::LevelFilter::Debug).unwrap();
    tui_logger::set_default_level(log::LevelFilter::Debug);

    let cli = Cli::parse();

    if let Err(e) = cli.validate() {
        error!("{e}");
        process::exit(1);
    }

    let mode = if cli.has_config_source() {
        if cli.should_use_memory_mode() {
            DatabaseMode::Memory
        } else {
            DatabaseMode::File
        }
    } else {
        DatabaseMode::File
    };

    if mode == DatabaseMode::File {
        init().await?;
        if let Err(e) = migrate_configs(None).await {
            error!("Database migration failed: {e}");
        }
    }

    if cli.has_config_source() {
        if let Err(e) = import_configs_from_source(&cli, mode).await {
            error!("Failed to import configs: {e}");
            process::exit(1);
        }
    }

    if cli.auto_start {
        let configs = read_configs_with_mode(mode).await.map_err(|e| {
            error!("Failed to read configs: {e}");
            e
        })?;

        if !configs.is_empty() {
            let config_ids: Vec<i64> = configs.into_iter().filter_map(|config| config.id).collect();

            if !config_ids.is_empty() {
                let mut tasks = FuturesUnordered::new();

                for config_id in config_ids {
                    tasks.push(async move {
                        if let Err(e) =
                            crate::core::port_forward::start_port_forward(config_id, mode).await
                        {
                            error!("Failed to start port forward for config {config_id}: {e}");
                        }
                    });
                }

                while let Some(()) = tasks.next().await {}
            }
        }
    }

    run_tui(mode).await
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
