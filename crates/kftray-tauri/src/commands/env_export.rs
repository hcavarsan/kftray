use kftray_commons::utils::env_export::generate_env_content;

/// Tauri command to export port-forward configurations as .env file content.
/// When `running_only` is true, only includes currently active port-forwards.
#[tauri::command]
pub async fn export_env_cmd(running_only: bool) -> Result<String, String> {
    generate_env_content(running_only).await
}
