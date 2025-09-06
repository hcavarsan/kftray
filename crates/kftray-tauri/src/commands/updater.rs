use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use kftray_commons::utils::settings::set_last_update_check;
use log::{
    error,
    info,
};
use tauri::{
    AppHandle,
    command,
};
use tauri_plugin_dialog::{
    DialogExt,
    MessageDialogButtons,
    MessageDialogKind,
};
use tauri_plugin_updater::UpdaterExt;

fn get_current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, String> {
    info!("Checking for application updates...");

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            error!("Failed to get updater: {}", e);
            return Err(format!("Failed to get updater: {}", e));
        }
    };

    let timestamp = get_current_timestamp();
    if let Err(e) = set_last_update_check(timestamp).await {
        error!("Failed to save last update check timestamp: {e}");
    }

    match updater.check().await {
        Ok(Some(update)) => {
            info!("Update available: version {}", update.version);

            let message = format!(
                "A new version {} is available!\n\nCurrent version: {}\nWould you like to update now?",
                update.version,
                app.package_info().version
            );

            let answer = app
                .dialog()
                .message(message)
                .title("Update Available")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::YesNo)
                .blocking_show();

            if answer {
                info!("User chose to install update");

                match update
                    .download_and_install(
                        |chunk_size, total_size| {
                            let progress = if let Some(total) = total_size {
                                format!("Downloaded {} / {} bytes", chunk_size, total)
                            } else {
                                format!("Downloaded {} bytes", chunk_size)
                            };
                            info!("{}", progress);
                        },
                        || {
                            info!("Download finished, installing...");
                        },
                    )
                    .await
                {
                    Ok(_) => {
                        info!("Update installed successfully");

                        let _ = app
                            .dialog()
                            .message(
                                "Update installed successfully! The application will restart now.",
                            )
                            .title("Update Complete")
                            .kind(MessageDialogKind::Info)
                            .blocking_show();

                        // Use process restart instead of app.restart() for more reliable restart
                        std::process::Command::new(std::env::current_exe().unwrap())
                            .spawn()
                            .expect("Failed to restart application");

                        std::process::exit(0);
                    }
                    Err(e) => {
                        error!("Failed to download or install update: {}", e);

                        let _ = app
                            .dialog()
                            .message(format!("Failed to install update: {}", e))
                            .title("Update Failed")
                            .kind(MessageDialogKind::Error)
                            .blocking_show();

                        Err(format!("Failed to install update: {}", e))
                    }
                }
            } else {
                info!("User declined to install update");
                Ok("Update declined by user".to_string())
            }
        }
        Ok(None) => {
            info!("Application is up to date");

            let _ = app
                .dialog()
                .message("You are running the latest version!")
                .title("No Updates")
                .kind(MessageDialogKind::Info)
                .blocking_show();

            Ok("Application is up to date".to_string())
        }
        Err(e) => {
            error!("Failed to check for updates: {}", e);

            let _ = app
                .dialog()
                .message(format!("Failed to check for updates: {}", e))
                .title("Update Check Failed")
                .kind(MessageDialogKind::Error)
                .blocking_show();

            Err(format!("Failed to check for updates: {}", e))
        }
    }
}

#[command]
pub async fn check_for_updates_silent(app: AppHandle) -> Result<bool, String> {
    info!("Silently checking for application updates...");

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            error!("Failed to get updater: {}", e);
            return Err(format!("Failed to get updater: {}", e));
        }
    };

    let timestamp = get_current_timestamp();
    if let Err(e) = set_last_update_check(timestamp).await {
        error!("Failed to save last update check timestamp: {e}");
    }

    match updater.check().await {
        Ok(Some(_update)) => {
            info!("Update available (silent check)");
            Ok(true)
        }
        Ok(None) => {
            info!("Application is up to date (silent check)");
            Ok(false)
        }
        Err(e) => {
            error!("Failed to check for updates (silent): {}", e);
            Err(format!("Failed to check for updates: {}", e))
        }
    }
}

#[command]
pub async fn install_update_silent(app: AppHandle) -> Result<String, String> {
    info!("Installing update silently...");

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            error!("Failed to get updater: {}", e);
            return Err(format!("Failed to get updater: {}", e));
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            info!(
                "Update available: version {}, installing silently",
                update.version
            );

            match update
                .download_and_install(
                    |chunk_size, total_size| {
                        let progress = if let Some(total) = total_size {
                            format!("Downloaded {} / {} bytes", chunk_size, total)
                        } else {
                            format!("Downloaded {} bytes", chunk_size)
                        };
                        info!("{}", progress);
                    },
                    || {
                        info!("Download finished, installing...");
                    },
                )
                .await
            {
                Ok(_) => {
                    info!("Update installed successfully, restarting app");

                    // Use process restart instead of app.restart() for more reliable restart
                    std::process::Command::new(std::env::current_exe().unwrap())
                        .spawn()
                        .expect("Failed to restart application");

                    std::process::exit(0);
                }
                Err(e) => {
                    error!("Failed to download or install update: {}", e);
                    Err(format!("Failed to install update: {}", e))
                }
            }
        }
        Ok(None) => {
            info!("No update available");
            Err("No update available".to_string())
        }
        Err(e) => {
            error!("Failed to check for updates: {}", e);
            Err(format!("Failed to check for updates: {}", e))
        }
    }
}

#[command]
pub async fn get_version_info(
    app: AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut info = std::collections::HashMap::new();

    info.insert(
        "current_version".to_string(),
        app.package_info().version.to_string(),
    );

    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(e) => {
            error!("Failed to get updater: {}", e);
            info.insert("update_available".to_string(), "unknown".to_string());
            info.insert("latest_version".to_string(), "unknown".to_string());
            return Ok(info);
        }
    };

    let timestamp = get_current_timestamp();
    if let Err(e) = set_last_update_check(timestamp).await {
        error!("Failed to save last update check timestamp: {e}");
    }

    match updater.check().await {
        Ok(Some(update)) => {
            info.insert("update_available".to_string(), "true".to_string());
            info.insert("latest_version".to_string(), update.version.to_string());
        }
        Ok(None) => {
            info.insert("update_available".to_string(), "false".to_string());
            info.insert(
                "latest_version".to_string(),
                app.package_info().version.to_string(),
            );
        }
        Err(e) => {
            error!("Failed to check for updates: {}", e);
            info.insert("update_available".to_string(), "error".to_string());
            info.insert("latest_version".to_string(), "unknown".to_string());
        }
    }

    Ok(info)
}
