use std::collections::HashSet;

use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::read_configs_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::{
    stop_port_forward_with_mode,
    stop_proxy_forward_with_mode,
};
use tokio::signal;

use crate::cli::args::Cli;
use crate::core;

pub struct PortForwardRunner;

impl PortForwardRunner {
    pub async fn auto_start_port_forwards(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let config_ids = Self::get_config_ids_to_start(cli, mode, imported_config_ids).await?;

        if config_ids.is_empty() {
            return Ok(Vec::new());
        }

        let successful_config_ids = Self::start_port_forwards(cli, mode, config_ids).await?;
        Ok(successful_config_ids)
    }

    pub async fn run_non_interactive_mode(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if cli.auto_start {
            let successful_config_ids =
                Self::auto_start_port_forwards(cli, mode, imported_config_ids).await?;
            Self::handle_auto_start_non_interactive(cli, mode, successful_config_ids).await
        } else {
            Self::handle_save_only_non_interactive().await
        }
    }

    async fn get_config_ids_to_start(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if cli.has_config_source() {
            Ok(imported_config_ids)
        } else {
            let configs = read_configs_with_mode(mode).await.map_err(|e| {
                eprintln!("Error: Failed to read configurations: {e}");
                e
            })?;
            Ok(configs.into_iter().filter_map(|config| config.id).collect())
        }
    }

    async fn start_port_forwards(
        cli: &Cli, mode: DatabaseMode, config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if cli.non_interactive {
            println!("Starting {} port forward(s)", config_ids.len());
        }

        let mut tasks = FuturesUnordered::new();
        let mut errors = Vec::new();

        for config_id in config_ids {
            tasks.push(async move {
                match core::port_forward::start_port_forward(config_id, mode).await {
                    Ok(()) => Ok(config_id),
                    Err(e) => {
                        eprintln!(
                            "Error: Failed to start port forward for config {config_id}: {e}"
                        );
                        Err((config_id, e))
                    }
                }
            });
        }

        let mut successful_config_ids = Vec::new();
        while let Some(result) = tasks.next().await {
            match result {
                Ok(config_id) => successful_config_ids.push(config_id),
                Err((config_id, e)) => errors.push((config_id, e)),
            }
        }

        if !errors.is_empty() {
            eprintln!("Warning: {} port forward(s) failed to start", errors.len());
            if cli.non_interactive {
                if successful_config_ids.is_empty() {
                    eprintln!("Error: All port forwards failed to start in non-interactive mode");
                    eprintln!("Check the errors above for details");
                    std::process::exit(1);
                } else {
                    eprintln!(
                        "Note: {} port forward(s) started successfully",
                        successful_config_ids.len()
                    );
                }
            }
        }

        Ok(successful_config_ids)
    }

    async fn handle_auto_start_non_interactive(
        _cli: &Cli, mode: DatabaseMode, successful_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let started_configs = Self::get_configs_by_ids(mode, successful_config_ids).await?;

        if started_configs.is_empty() {
            eprintln!("Error: No configurations found");
            eprintln!("Ensure config source contains valid port forward configurations");
            std::process::exit(1);
        }

        Self::print_active_configurations(&started_configs);
        Self::wait_for_shutdown_signal(&started_configs, mode).await;
        Ok(())
    }

    async fn handle_save_only_non_interactive() -> Result<(), Box<dyn std::error::Error>> {
        println!("Configurations processed");
        Ok(())
    }

    async fn get_configs_by_ids(
        mode: DatabaseMode, config_ids: Vec<i64>,
    ) -> Result<Vec<Config>, Box<dyn std::error::Error>> {
        let all_configs = read_configs_with_mode(mode).await.map_err(|e| {
            eprintln!("Error: Failed to read configurations: {e}");
            Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>
        })?;

        let target_ids: HashSet<i64> = config_ids.into_iter().collect();
        Ok(all_configs
            .into_iter()
            .filter(|config| target_ids.contains(&config.id.unwrap_or(-1)))
            .collect())
    }

    fn print_active_configurations(configs: &[Config]) {
        println!("Port forwards started");
        println!("Active configurations: {}", configs.len());

        for config in configs {
            let alias = config.alias.as_deref().unwrap_or("unnamed");
            let local_port = config.local_port.unwrap_or(0);
            let remote_port = config.remote_port.unwrap_or(0);
            let namespace = &config.namespace;
            let service_or_target = match config.workload_type.as_deref() {
                Some("proxy") => config.remote_address.as_deref().unwrap_or("unknown"),
                Some("pod") => config.target.as_deref().unwrap_or("unknown"),
                Some("service") => config.service.as_deref().unwrap_or("unknown"),
                _ => config.service.as_deref().unwrap_or("unknown"),
            };

            println!(
                "  {alias} {local_port}:{remote_port} -> {service_or_target}:{remote_port} ({namespace})"
            );
        }

        println!("\nRunning in non-interactive mode (press Ctrl+C to stop)");
        println!("Keeping port forwards active");
    }

    async fn wait_for_shutdown_signal(configs: &[Config], mode: DatabaseMode) {
        let ctrl_c = signal::ctrl_c();

        tokio::select! {
            _ = ctrl_c => {
                println!("\nStopping port forwards");
                Self::stop_all_port_forwards(configs, mode).await;
            }
        }
    }

    async fn stop_all_port_forwards(configs: &[Config], mode: DatabaseMode) {
        let mut stop_errors = Vec::new();
        let mut stopped_count = 0;

        for config in configs {
            if let Some(config_id) = config.id {
                let stop_result = Self::stop_single_port_forward(config, config_id, mode).await;

                match stop_result {
                    Ok(()) => stopped_count += 1,
                    Err(e) => stop_errors.push(format!("Config {config_id}: {e}")),
                }
            }
        }

        println!("Stopped {stopped_count} port forward(s)");

        if stop_errors.is_empty() {
            println!("Port forwards stopped");
        } else {
            eprintln!("Warning: Some port forwards failed to stop properly");
            for error in stop_errors {
                eprintln!("  {error}");
            }
        }
    }

    async fn stop_single_port_forward(
        config: &Config, config_id: i64, mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match config.workload_type.as_deref() {
            Some("proxy") => {
                let namespace = &config.namespace;
                let service_name = config
                    .service
                    .clone()
                    .unwrap_or_else(|| format!("proxy-{config_id}"));
                stop_proxy_forward_with_mode(config_id, namespace, service_name, mode)
                    .await
                    .map(|_| ())
                    .map_err(|e| Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>)
            }
            _ => stop_port_forward_with_mode(config_id.to_string(), mode)
                .await
                .map(|_| ())
                .map_err(|e| Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>),
        }
    }
}
