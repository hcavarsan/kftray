pub mod protocol;
pub mod server;
pub mod tools;

pub async fn init_database() -> anyhow::Result<()> {
    kftray_commons::utils::db::init()
        .await
        .map_err(|e| anyhow::anyhow!("database initialization failed: {e}"))?;
    kftray_commons::utils::migration::migrate_configs(None)
        .await
        .map_err(|e| anyhow::anyhow!("database migration failed: {e}"))
}
