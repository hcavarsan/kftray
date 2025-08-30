use std::env;

use anyhow::Result;
use kube::config::Kubeconfig;
use kube::Client;
use log::info;

use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

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
                    errors.push(format!(
                        "Failed to create client for context '{context_name}': All connection strategies failed"
                    ));
                }
            },
            Err(e) => {
                errors.push(format!(
                    "Failed to create configuration for context '{context_name}': {e}. Check if the context exists and is properly configured"
                ));
            }
        }
    } else {
        info!("No specific context provided, returning all available contexts.");
        return Ok((None, None, all_contexts));
    }

    Err(anyhow::anyhow!(
        "Unable to create Kubernetes client. Tried {} kubeconfig path(s). Errors encountered:\n{}",
        kubeconfig_paths.len(),
        errors
            .iter()
            .map(|e| format!("  • {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
