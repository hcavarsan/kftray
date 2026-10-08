use kftray_commons::utils::settings;
use log::error;

#[tauri::command]
pub async fn get_telemetry_enabled() -> Result<Option<bool>, String> {
    settings::get_telemetry_enabled().await.map_err(|e| {
        error!("Failed to get telemetry enabled: {e}");
        format!("Failed to get telemetry enabled: {e}")
    })
}

#[tauri::command]
pub async fn update_telemetry_enabled(enabled: bool) -> Result<(), String> {
    settings::set_telemetry_enabled(enabled)
        .await
        .map_err(|e| {
            error!("Failed to update telemetry enabled: {e}");
            format!("Failed to update telemetry enabled: {e}")
        })?;
    crate::telemetry::set_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub fn report_error(name: String, message: String, stack: Option<String>) {
    crate::telemetry::capture_frontend_error(name, message, stack);
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

        update_telemetry_enabled(true).await.unwrap();
        assert_eq!(get_telemetry_enabled().await.unwrap(), Some(true));

        update_telemetry_enabled(false).await.unwrap();
        assert_eq!(get_telemetry_enabled().await.unwrap(), Some(false));
    }
}
