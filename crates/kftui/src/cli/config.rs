use std::fs;

use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::{
    parse_import_configs,
    upsert_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{
    GitHubConfig,
    GitHubRepository,
};
use kftray_portforward::kube::{
    retrieve_service_configs,
    stop_all_port_forward_with_mode,
};

use crate::cli::args::Cli;
use crate::stdin;

pub struct ConfigImporter;

impl ConfigImporter {
    pub async fn import_configs(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
        Self::print_import_start_message(cli, mode);

        let configs = Self::load_from_source(cli).await?;

        Self::handle_flush_if_needed(cli, mode).await?;

        upsert_configs_with_mode(configs, mode)
            .await
            .map_err(|e| format!("Failed to save configs to database: {e}"))?;

        Self::print_import_success_message(cli, mode);

        Ok(())
    }

    async fn handle_flush_if_needed(cli: &Cli, mode: DatabaseMode) -> Result<(), String> {
        if cli.flush && mode == DatabaseMode::File {
            if cli.non_interactive {
                println!("Clearing existing configurations");
            }

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

        let (mode_text, location_text) = Self::get_mode_text(mode);

        if cli.auto_discover {
            println!(
                "{} configurations from Kubernetes annotations (context: {}) {}",
                mode_text,
                cli.context.as_deref().unwrap(),
                location_text
            );
        } else if cli.is_github_import() {
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
        if cli.auto_discover {
            "annotations"
        } else if cli.is_github_import() {
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

    async fn load_from_source(cli: &Cli) -> Result<Vec<Config>, String> {
        if cli.auto_discover {
            Self::load_from_annotations(cli).await
        } else if cli.is_github_import() {
            Self::load_from_github(cli)
        } else if let Some(config_path) = cli.get_config_path() {
            Self::load_from_file(config_path)
        } else if let Some(json_content) = cli.get_json() {
            parse_import_configs(json_content)
                .map_err(|e| format!("Failed to import configs from JSON: {e}"))
        } else if cli.stdin {
            Self::load_from_stdin()
        } else {
            Err("No config source specified".to_string())
        }
    }

    async fn load_from_annotations(cli: &Cli) -> Result<Vec<Config>, String> {
        let context = cli.context.as_deref().unwrap();

        if cli.non_interactive {
            println!("Discovering annotated services from context: {context}");
        }

        let mut configs = retrieve_service_configs(context, cli.kubeconfig.clone())
            .await
            .map_err(|e| {
                format!("Failed to retrieve annotated services from context '{context}': {e}")
            })?;

        for config in &mut configs {
            config.domain_enabled = Some(cli.alias_as_domain);
            config.auto_loopback_address = cli.auto_loopback;
        }

        Ok(configs)
    }

    fn load_from_github(cli: &Cli) -> Result<Vec<Config>, String> {
        let github_url = cli.get_github_url().unwrap();
        let config_paths = cli
            .get_configs_path_with_default()
            .split(',')
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect();
        let github_token = std::env::var("GITHUB_TOKEN").ok();

        let github_config = GitHubConfig {
            repo_url: github_url.to_string(),
            config_paths,
            use_system_credentials: true,
            github_token,
            flush_existing: false,
        };

        GitHubRepository::fetch_config_content(&github_config)
            .and_then(|content| parse_import_configs(&content))
            .map_err(|e| {
                format!("Failed to import configs from GitHub repository '{github_url}': {e}")
            })
    }

    fn load_from_file(config_path: &str) -> Result<Vec<Config>, String> {
        let json_content = fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file '{config_path}': {e}"))?;

        parse_import_configs(&json_content)
            .map_err(|e| format!("Failed to import configs from file '{config_path}': {e}"))
    }

    fn load_from_stdin() -> Result<Vec<Config>, String> {
        let stdin_content =
            stdin::read_stdin_content().map_err(|e| format!("Failed to read from stdin: {e}"))?;

        parse_import_configs(&stdin_content)
            .map_err(|e| format!("Failed to import configs from stdin: {e}"))
    }
}
