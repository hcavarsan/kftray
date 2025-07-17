use std::fs;

use kftray_commons::utils::config::import_configs_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{
    GitHubConfig,
    GitHubRepository,
};
use kftray_portforward::kube::stop_all_port_forward_with_mode;

use crate::cli::args::Cli;
use crate::stdin;

pub struct ConfigImporter;

impl ConfigImporter {
    pub async fn import_configs(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
        Self::handle_flush_if_needed(cli, mode).await?;
        Self::print_import_start_message(cli, mode);

        let result = Self::import_from_source(cli, mode).await;

        if result.is_ok() {
            Self::print_import_success_message(cli, mode);
        }

        result
    }

    async fn handle_flush_if_needed(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
        if cli.flush && mode == DatabaseMode::File {
            if let Err(e) =
                kftray_commons::utils::github::clear_existing_configs_with_mode(mode).await
            {
                return Err(format!("Failed to clear existing configs: {e}"));
            }

            if let Err(e) = stop_all_port_forward_with_mode(mode).await {
                eprintln!("Warning: Failed to stop all port forwards during flush: {e:?}");
            }
        }
        Ok(())
    }

    fn print_import_start_message(cli: &Cli, mode: DatabaseMode) {
        if !cli.non_interactive {
            return;
        }

        if cli.flush {
            println!("Clearing existing configurations");
        }

        let (mode_text, location_text) = Self::get_mode_text(mode);

        if cli.is_github_import() {
            println!(
                "{} configurations from GitHub: {} {}",
                mode_text,
                cli.get_github_url().unwrap(),
                location_text
            );
        } else if let Some(config_path) = cli.get_config_path() {
            println!("{mode_text} configurations from file: {config_path} {location_text}");
        } else if cli.get_json().is_some() {
            println!("{mode_text} configurations from JSON {location_text}");
        } else if cli.stdin {
            println!("{mode_text} configurations from stdin {location_text}");
        }
    }

    fn print_import_success_message(cli: &Cli, mode: DatabaseMode) {
        if !cli.non_interactive {
            return;
        }

        let action_text = Self::get_action_text(mode);
        println!(
            "Configurations {} from {}",
            action_text,
            Self::get_source_description(cli)
        );
    }

    fn get_mode_text(mode: DatabaseMode) -> (&'static str, &'static str) {
        if mode == DatabaseMode::Memory {
            ("Loading", "into memory")
        } else {
            ("Importing", "to database")
        }
    }

    fn get_action_text(mode: DatabaseMode) -> &'static str {
        if mode == DatabaseMode::Memory {
            "loaded"
        } else {
            "imported"
        }
    }

    fn get_source_description(cli: &Cli) -> &'static str {
        if cli.is_github_import() {
            "GitHub"
        } else if cli.get_config_path().is_some() {
            "file"
        } else if cli.get_json().is_some() {
            "JSON"
        } else if cli.stdin {
            "stdin"
        } else {
            "unknown"
        }
    }

    async fn import_from_source(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
        if cli.is_github_import() {
            Self::import_from_github(cli, mode).await
        } else if let Some(config_path) = cli.get_config_path() {
            Self::import_from_file(config_path, mode).await
        } else if let Some(json_content) = cli.get_json() {
            Self::import_from_json(json_content, mode).await
        } else if cli.stdin {
            Self::import_from_stdin(mode).await
        } else {
            Err("No config source specified".to_string())
        }
    }

    async fn import_from_github(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
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

        GitHubRepository::import_configs(github_config, mode)
            .await
            .map_err(|e| {
                format!("Failed to import configs from GitHub repository '{github_url}': {e}")
            })
    }

    async fn import_from_file(config_path: &str, mode: DatabaseMode) -> Result<(), String> {
        let json_content = fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file '{config_path}': {e}"))?;

        import_configs_with_mode(json_content, mode)
            .await
            .map_err(|e| format!("Failed to import configs from file '{config_path}': {e}"))
    }

    async fn import_from_json(json_content: &str, mode: DatabaseMode) -> Result<(), String> {
        import_configs_with_mode(json_content.to_string(), mode)
            .await
            .map_err(|e| format!("Failed to import configs from JSON: {e}"))
    }

    async fn import_from_stdin(mode: DatabaseMode) -> Result<(), String> {
        let stdin_content =
            stdin::read_stdin_content().map_err(|e| format!("Failed to read from stdin: {e}"))?;

        import_configs_with_mode(stdin_content, mode)
            .await
            .map_err(|e| format!("Failed to import configs from stdin: {e}"))
    }
}
