use kftray_commons::models::config_model::Config;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::ssh_forward::{
    SshTestStep,
    forget_host_key,
    test_connection,
};

/// Runs every stage of an ssh-port-forward config on a fresh session,
/// without starting it, and reports each step.
#[tauri::command]
pub async fn test_ssh_forward_cmd(config: Config) -> Result<Vec<SshTestStep>, String> {
    Ok(test_connection(&config, DatabaseMode::File).await)
}

/// Forgets the stored host key of the config's bastion.
#[tauri::command]
pub async fn forget_ssh_host_key_cmd(config: Config) -> Result<(), String> {
    forget_host_key(&config, DatabaseMode::File).await
}
