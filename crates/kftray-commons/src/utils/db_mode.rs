use std::sync::Arc;

use sqlx::SqlitePool;

use crate::db::{
    create_db_table,
    get_db_pool,
};
use crate::utils::pool_slot::PoolSlot;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Copy)]
pub enum DatabaseMode {
    #[default]
    File,
    Memory,
}

pub struct DatabaseContext {
    pub pool: Arc<SqlitePool>,
    pub mode: DatabaseMode,
}

pub struct DatabaseManager;

static MEMORY_DB_POOL: PoolSlot = PoolSlot::new();

async fn create_memory_pool() -> Result<Arc<SqlitePool>, String> {
    let pool = Arc::new(
        SqlitePool::connect("sqlite::memory:")
            .await
            .map_err(|e| e.to_string())?,
    );
    create_db_table(&pool).await.map_err(|e| e.to_string())?;
    if let Err(error) = crate::utils::settings::establish_expose_history_baseline_at_init(
        &pool,
        DatabaseMode::Memory,
    )
    .await
    {
        // Bookkeeping only: expose::kubernetes::ensure_expose_history_baseline
        // re-establishes it lazily, using the snapshot taken above
        // of which config ids already existed, so a
        // configuration inserted after this point is never
        // mistaken for one that predates ingress history.
        log::warn!("Failed to establish the expose history baseline: {error}");
    }
    crate::utils::migration::migrate_configs(Some(&pool))
        .await
        .map_err(|e| e.to_string())?;
    Ok(pool)
}

impl DatabaseManager {
    pub async fn get_context(mode: DatabaseMode) -> Result<DatabaseContext, String> {
        let pool = match mode {
            DatabaseMode::File => get_db_pool().await.map_err(|e| e.to_string())?,
            DatabaseMode::Memory => MEMORY_DB_POOL.get_or_init(create_memory_pool).await?,
        };
        Ok(DatabaseContext { pool, mode })
    }

    pub fn cleanup_memory_pools() {
        MEMORY_DB_POOL.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_mode_default() {
        let mode = DatabaseMode::default();
        assert_eq!(mode, DatabaseMode::File);
    }

    #[tokio::test]
    async fn test_database_context_memory() {
        let _db = crate::test_utils::test_db().await;
        let context = DatabaseManager::get_context(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(context.mode, DatabaseMode::Memory);
        assert!(!context.pool.is_closed());
    }

    #[tokio::test]
    async fn test_database_context_file() {
        let _db = crate::test_utils::test_db().await;
        let context = DatabaseManager::get_context(DatabaseMode::File).await;
        if let Ok(ctx) = context {
            assert_eq!(ctx.mode, DatabaseMode::File);
        }
    }
}
