use std::sync::Arc;
use std::sync::{
    LazyLock,
    Mutex,
};

use sqlx::SqlitePool;

use crate::db::{
    create_db_table,
    get_db_pool,
};

#[derive(Debug, Clone, PartialEq, Default, Copy)]
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

static MEMORY_DB_POOL: LazyLock<Mutex<Option<Arc<SqlitePool>>>> =
    LazyLock::new(|| Mutex::new(None));

impl DatabaseManager {
    pub async fn get_context(mode: DatabaseMode) -> Result<DatabaseContext, String> {
        match mode {
            DatabaseMode::File => {
                let pool = get_db_pool().await.map_err(|e| e.to_string())?;
                Ok(DatabaseContext { pool, mode })
            }
            DatabaseMode::Memory => {
                {
                    let pool_guard = MEMORY_DB_POOL.lock().unwrap();
                    if let Some(pool) = pool_guard.as_ref() {
                        return Ok(DatabaseContext {
                            pool: pool.clone(),
                            mode,
                        });
                    }
                }

                let connection_string = "sqlite::memory:";

                let pool = Arc::new(
                    SqlitePool::connect(connection_string)
                        .await
                        .map_err(|e| e.to_string())?,
                );
                create_db_table(&pool).await.map_err(|e| e.to_string())?;

                {
                    let mut pool_guard = MEMORY_DB_POOL.lock().unwrap();
                    *pool_guard = Some(pool.clone());
                }

                Ok(DatabaseContext { pool, mode })
            }
        }
    }

    pub fn cleanup_memory_pools() {
        let mut pool_guard = MEMORY_DB_POOL.lock().unwrap();
        *pool_guard = None;
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
        let context = DatabaseManager::get_context(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(context.mode, DatabaseMode::Memory);
        assert!(!context.pool.is_closed());
    }

    #[tokio::test]
    async fn test_database_context_file() {
        let context = DatabaseManager::get_context(DatabaseMode::File).await;
        if let Ok(ctx) = context {
            assert_eq!(ctx.mode, DatabaseMode::File);
        }
    }
}
