use std::collections::HashSet;

use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::read_configs_with_mode;
use kftray_commons::utils::db::init as init_db;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::migration::migrate_configs;

use crate::cli::args::Cli;
use crate::cli::config::ConfigImporter;
use crate::cli::runner::PortForwardRunner;
use crate::logging::LoggerState;
use crate::stdin;
use crate::tui::run_tui;

pub struct CliHandler {
    cli: Cli,
    mode: DatabaseMode,
    logger_state: LoggerState,
}

impl CliHandler {
    pub fn new(cli: Cli, logger_state: LoggerState) -> Self {
        let mode = Self::determine_database_mode(&cli);
        Self {
            cli,
            mode,
            logger_state,
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_args()?;
        self.initialize_database().await?;

        let imported_config_ids = self.handle_config_import().await?;

        if self.cli.auto_start && !self.cli.non_interactive {
            let _successful_config_ids = PortForwardRunner::auto_start_port_forwards(
                &self.cli,
                self.mode,
                imported_config_ids.clone(),
            )
            .await?;
        }

        self.handle_stdin_redirect().await?;
        self.handle_execution_mode(imported_config_ids).await
    }

    fn determine_database_mode(cli: &Cli) -> DatabaseMode {
        if cli.has_config_source() && cli.should_use_memory_mode() {
            DatabaseMode::Memory
        } else {
            DatabaseMode::File
        }
    }

    fn validate_args(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = self.cli.validate() {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
        Ok(())
    }

    async fn initialize_database(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.mode != DatabaseMode::File {
            return Ok(());
        }

        if self.cli.non_interactive {
            println!("Initializing database");
        }

        if let Err(e) = init_db().await {
            eprintln!("Error: Database initialization failed: {e}");
            return Err(e);
        }

        if let Err(e) = migrate_configs(None).await {
            eprintln!("Error: Database migration failed: {e}");
            return Err(e.into());
        }

        if self.cli.non_interactive {
            println!("Database initialized");
        }

        Ok(())
    }

    async fn handle_config_import(&self) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if !self.cli.has_config_source() {
            return Ok(Vec::new());
        }

        let configs_before = if self.cli.auto_start && !self.cli.flush {
            self.get_existing_configs().await?
        } else {
            Vec::new()
        };

        if let Err(e) = ConfigImporter::import_configs(&self.cli, self.mode).await {
            eprintln!("Error: Failed to import configurations: {e}");
            std::process::exit(1);
        }

        if self.cli.auto_start {
            self.calculate_imported_config_ids(configs_before).await
        } else {
            Ok(Vec::new())
        }
    }

    async fn get_existing_configs(&self) -> Result<Vec<Config>, Box<dyn std::error::Error>> {
        read_configs_with_mode(self.mode).await.map_err(|e| {
            eprintln!("Error: Failed to read configurations before import: {e}");
            Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>
        })
    }

    async fn calculate_imported_config_ids(
        &self, configs_before: Vec<Config>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let configs_after = read_configs_with_mode(self.mode).await.map_err(|e| {
            eprintln!("Error: Failed to read configurations after import: {e}");
            e
        })?;

        let imported_ids = if self.cli.flush {
            configs_after
                .into_iter()
                .filter_map(|config| config.id)
                .collect()
        } else {
            let before_ids: HashSet<i64> = configs_before
                .into_iter()
                .filter_map(|config| config.id)
                .collect();

            configs_after
                .into_iter()
                .filter_map(|config| config.id)
                .filter(|id| !before_ids.contains(id))
                .collect()
        };

        Ok(imported_ids)
    }

    async fn handle_stdin_redirect(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.cli.stdin {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            if let Err(e) = stdin::redirect_stdin_to_tty() {
                eprintln!("Error: Failed to redirect stdin to tty: {e}");
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_execution_mode(
        &self, imported_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.cli.non_interactive {
            PortForwardRunner::run_non_interactive_mode(&self.cli, self.mode, imported_config_ids)
                .await
        } else {
            run_tui(self.mode, self.logger_state.clone()).await
        }
    }
}
