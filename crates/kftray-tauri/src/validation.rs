use kftray_commons::utils::validate_configs::{
    ConfigLocation,
    detect_multiple_configs,
    format_alert_message,
};
use tauri::{
    AppHandle,
    Manager,
    Runtime,
    async_runtime::spawn_blocking,
};
use tauri_plugin_dialog::{
    DialogExt,
    MessageDialogButtons,
};

async fn show_alert_dialog<R: Runtime>(
    app_handle: AppHandle<R>, configs: Vec<ConfigLocation>, active_config: Option<ConfigLocation>,
) {
    let full_message = format_alert_message(configs, active_config);

    let app_handle_clone = app_handle.clone();
    spawn_blocking(move || {
        let app_handle_inner = app_handle_clone.clone();
        let _ = app_handle_clone.run_on_main_thread(move || {
            if let Some(window) = app_handle_inner.get_webview_window("main") {
                window
                    .dialog()
                    .message(&full_message)
                    .title("Multiple Configuration Directories Detected")
                    .buttons(MessageDialogButtons::Ok)
                    .show(move |_response| {
                        // User acknowledged the warning
                    });
            }
        });
    })
    .await
    .unwrap();
}

pub async fn alert_multiple_configs<R: Runtime>(app_handle: AppHandle<R>) {
    let (configs, active_config) = detect_multiple_configs();
    if configs.len() > 1 {
        show_alert_dialog(app_handle, configs, active_config).await;
    }
}
