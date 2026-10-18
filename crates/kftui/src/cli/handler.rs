use kftray_commons::utils::config_view::get_config_view_with_mode;
use kftray_commons::utils::db::init as init_db;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::migration::migrate_configs;

use crate::cli::args::Cli;
use crate::cli::config::ConfigImporter;
use crate::cli::output;
use crate::cli::runner::PortForwardRunner;
use crate::cli::watch::DiscoveryWatch;
use crate::logging::LoggerState;
#[cfg(target_os = "macos")]
use crate::stdin;
use crate::tui::run_tui;

pub struct CliHandler {
    cli: Cli,
    mode: DatabaseMode,
    logger_state: LoggerState,
}

impl CliHandler {
    pub fn new(mut cli: Cli, logger_state: LoggerState) -> Self {
        cli.non_interactive |= cli.runs_command();
        let mode = Self::determine_database_mode(&cli);
        Self {
            cli,
            mode,
            logger_state,
        }
    }

    pub async fn run(self) -> Result<Option<i32>, Box<dyn std::error::Error>> {
        self.validate_args()?;
        self.initialize_database().await?;
        kftray_telemetry::load_setting_with_mode(self.mode).await;

        let imported_config_ids = self.handle_config_import().await?;

        self.prepare_tui_forwards(imported_config_ids.clone())
            .await?;

        self.handle_stdin_redirect().await?;
        self.handle_execution_mode(imported_config_ids).await
    }

    pub(crate) async fn prepare_tui_forwards(
        &self, imported_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.cli.non_interactive {
            return Ok(());
        }
        if self.cli.auto_start {
            PortForwardRunner::auto_start_port_forwards(&self.cli, self.mode, imported_config_ids)
                .await?;
        } else if self.cli.ssl {
            PortForwardRunner::ensure_ssl_for_saved_configs(self.mode).await?;
        }
        Ok(())
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
            output::error!("Error: {e}");
            std::process::exit(1);
        }
        Ok(())
    }

    async fn initialize_database(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.mode != DatabaseMode::File {
            return Ok(());
        }

        if self.cli.non_interactive {
            output::status!("Initializing database");
        }

        if let Err(e) = init_db().await {
            output::error!("Error: Database initialization failed: {e}");
            return Err(e);
        }

        if let Err(e) = migrate_configs(None).await {
            output::error!("Error: Database migration failed: {e}");
            return Err(e.into());
        }

        if self.cli.non_interactive {
            output::status!("Database initialized");
        }

        Ok(())
    }

    async fn handle_config_import(&self) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if !self.cli.has_config_source() {
            return Ok(Vec::new());
        }

        match ConfigImporter::import_configs(&self.cli, self.mode).await {
            Ok(ids) if self.cli.auto_start => Ok(ids),
            Ok(_) => Ok(Vec::new()),
            Err(e) => {
                output::error!("Error: Failed to import configurations: {e}");
                std::process::exit(1);
            }
        }
    }

    pub(crate) async fn handle_stdin_redirect(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.cli.stdin || self.cli.non_interactive {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            if let Err(e) = stdin::redirect_stdin_to_tty() {
                output::error!("Error: Failed to redirect stdin to tty: {e}");
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_execution_mode(
        &self, imported_config_ids: Vec<i64>,
    ) -> Result<Option<i32>, Box<dyn std::error::Error>> {
        if self.cli.non_interactive {
            PortForwardRunner::run_non_interactive_mode(&self.cli, self.mode, imported_config_ids)
                .await
        } else {
            let (config_view, saved_group_by) = self
                .cli
                .tui_view(get_config_view_with_mode(self.mode).await);
            let discovery = self
                .cli
                .watch
                .then(|| DiscoveryWatch::spawn(self.cli.clone(), self.mode));
            run_tui(
                self.mode,
                self.logger_state.clone(),
                self.cli.no_update_check,
                config_view,
                saved_group_by,
                discovery,
            )
            .await?;
            Ok(None)
        }
    }
}
