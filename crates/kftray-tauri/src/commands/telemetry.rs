use kftray_commons::utils::settings;
use log::error;
use tauri::{
    AppHandle,
    Emitter,
    Wry,
};

#[tauri::command]
pub async fn get_telemetry_enabled() -> Result<Option<bool>, String> {
    settings::get_telemetry_enabled().await.map_err(|e| {
        error!("Failed to get telemetry enabled: {e}");
        format!("Failed to get telemetry enabled: {e}")
    })
}

/// Every webview runs its own copy of the crash-reporting SDK, so the new
/// choice is broadcast: the logs window must stop reporting the moment the
/// switch is turned off in the main window.
pub const TELEMETRY_ENABLED_CHANGED: &str = "telemetry-enabled-changed";

#[tauri::command]
pub async fn update_telemetry_enabled(
    app_handle: AppHandle<Wry>, enabled: bool,
) -> Result<(), String> {
    store_telemetry_enabled(enabled).await?;
    if let Err(e) = app_handle.emit(TELEMETRY_ENABLED_CHANGED, enabled) {
        error!("Failed to emit telemetry enabled change: {e}");
    }
    Ok(())
}

async fn store_telemetry_enabled(enabled: bool) -> Result<(), String> {
    settings::set_telemetry_enabled(enabled)
        .await
        .map_err(|e| {
            error!("Failed to update telemetry enabled: {e}");
            format!("Failed to update telemetry enabled: {e}")
        })?;
    kftray_telemetry::set_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub async fn get_performance_enabled() -> Result<Option<bool>, String> {
    settings::get_performance_enabled().await.map_err(|e| {
        error!("Failed to get performance enabled: {e}");
        format!("Failed to get performance enabled: {e}")
    })
}

/// Broadcast for the same reason as `TELEMETRY_ENABLED_CHANGED`: every
/// webview samples its own spans.
pub const PERFORMANCE_ENABLED_CHANGED: &str = "performance-enabled-changed";

#[tauri::command]
pub async fn update_performance_enabled(
    app_handle: AppHandle<Wry>, enabled: bool,
) -> Result<(), String> {
    store_performance_enabled(enabled).await?;
    if let Err(e) = app_handle.emit(PERFORMANCE_ENABLED_CHANGED, enabled) {
        error!("Failed to emit performance enabled change: {e}");
    }
    Ok(())
}

async fn store_performance_enabled(enabled: bool) -> Result<(), String> {
    settings::set_performance_enabled(enabled)
        .await
        .map_err(|e| {
            error!("Failed to update performance enabled: {e}");
            format!("Failed to update performance enabled: {e}")
        })?;
    kftray_telemetry::set_performance_enabled(enabled);
    Ok(())
}

/// Hands the webview what it needs to report crashes the way this process
/// does: same DSN gate, release, build target and run id. Read once at
/// startup; consent changes afterwards go through `update_telemetry_enabled`.
#[tauri::command]
pub fn get_telemetry_context() -> kftray_telemetry::FrontendContext {
    kftray_telemetry::frontend_context()
}

#[tauri::command]
pub async fn submit_problem_report(
    report: kftray_telemetry::problem_reports::ProblemReport,
) -> Result<(), String> {
    kftray_telemetry::problem_reports::submit(report)
        .await
        .map_err(|e| {
            error!("Failed to submit problem report: {e}");
            e.to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn use_test_db() -> kftray_commons::test_utils::TestDb {
        let db = kftray_commons::test_utils::test_db().await;
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        kftray_commons::utils::db::create_db_table(&pool)
            .await
            .unwrap();
        kftray_commons::utils::db::set_db_pool(std::sync::Arc::new(pool));
        db
    }

    #[tokio::test]
    async fn the_choice_is_unset_until_the_user_answers() {
        let _db = use_test_db().await;

        assert_eq!(get_telemetry_enabled().await.unwrap(), None);

        store_telemetry_enabled(true).await.unwrap();
        assert_eq!(get_telemetry_enabled().await.unwrap(), Some(true));

        store_telemetry_enabled(false).await.unwrap();
        assert_eq!(get_telemetry_enabled().await.unwrap(), Some(false));
    }

    #[tokio::test]
    async fn the_performance_choice_is_unset_until_the_user_answers() {
        let _db = use_test_db().await;

        assert_eq!(get_performance_enabled().await.unwrap(), None);

        store_performance_enabled(true).await.unwrap();
        assert_eq!(get_performance_enabled().await.unwrap(), Some(true));

        store_performance_enabled(false).await.unwrap();
        assert_eq!(get_performance_enabled().await.unwrap(), Some(false));
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn problem_reports_are_refused_in_debug_builds() {
        let report = serde_json::from_value(serde_json::json!({
            "report_id": "6f1c0f3e-8a53-4c0e-9a39-2f4c8f4f7b11",
            "message": "it broke",
            "email": null,
            "event_id": null,
        }))
        .unwrap();

        let result = submit_problem_report(report).await;

        assert_eq!(
            result,
            Err("Problem reporting is turned off for this build or environment".to_owned())
        );
    }
}
