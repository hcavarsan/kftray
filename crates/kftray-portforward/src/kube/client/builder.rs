use std::env;

use anyhow::Result;
use kube::Client;
use kube::config::Kubeconfig;
use log::info;

use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

fn env_debug_info() -> String {
    let path = env::var("PATH")
        .map(|p| {
            if p.len() > 80 {
                format!("{}...", &p[..80])
            } else {
                p
            }
        })
        .unwrap_or_else(|_| "<not set>".into());
    let home = env::var("HOME").unwrap_or_else(|_| "<not set>".into());
    let kubeconfig = env::var("KUBECONFIG").unwrap_or_else(|_| "<not set>".into());

    format!("PATH={path} | HOME={home} | KUBECONFIG={kubeconfig}")
}

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    {
        unsafe { env::remove_var("PYTHONHOME") };

        unsafe { env::remove_var("PYTHONPATH") };
    }

    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, all_contexts, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    if let Some(context_name) = context_name {
        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => match create_client_with_config(&config).await {
                Some(client) => {
                    info!("Created new client for context: {context_name}");
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                }
                _ => {
                    errors.push(format!("Connection failed for context '{context_name}'"));
                }
            },
            Err(e) => {
                errors.push(format!("Config error for context '{context_name}': {e}"));
            }
        }
    } else {
        info!("No specific context provided, returning all available contexts.");
        return Ok((None, None, all_contexts));
    }

    Err(anyhow::anyhow!(
        "Failed to create Kubernetes client.\n\
         Errors:\n{}\n\
         Environment: {}",
        errors
            .iter()
            .map(|e| format!("  • {e}"))
            .collect::<Vec<_>>()
            .join("\n"),
        env_debug_info()
    ))
}
