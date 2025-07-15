use std::collections::HashMap;
use std::sync::{
    Arc,
    Weak,
};
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

static MEMORY_DB_POOLS: LazyLock<Mutex<HashMap<std::thread::ThreadId, Weak<SqlitePool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl DatabaseManager {
    pub async fn get_context(mode: DatabaseMode) -> Result<DatabaseContext, String> {
        match mode {
            DatabaseMode::File => {
                let pool = get_db_pool().await.map_err(|e| e.to_string())?;
                Ok(DatabaseContext { pool, mode })
            }
            DatabaseMode::Memory => {
                let thread_id = std::thread::current().id();

                {
                    let mut pools = MEMORY_DB_POOLS.lock().unwrap();

                    pools.retain(|_, weak_pool| weak_pool.strong_count() > 0);

                    if let Some(weak_pool) = pools.get(&thread_id) {
                        if let Some(pool) = weak_pool.upgrade() {
                            return Ok(DatabaseContext { pool, mode });
                        } else {
                            pools.remove(&thread_id);
                        }
                    }
                }

                let db_name = format!("test_db_{thread_id:?}");
                let connection_string = format!("sqlite:file:{db_name}?mode=memory&cache=shared");

                let pool = Arc::new(
                    SqlitePool::connect(&connection_string)
                        .await
                        .map_err(|e| e.to_string())?,
                );
                create_db_table(&pool).await.map_err(|e| e.to_string())?;

                {
                    let mut pools = MEMORY_DB_POOLS.lock().unwrap();
                    pools.insert(thread_id, Arc::downgrade(&pool));
                }

                Ok(DatabaseContext { pool, mode })
            }
        }
    }

    pub fn cleanup_memory_pools() {
        let mut pools = MEMORY_DB_POOLS.lock().unwrap();
        pools.retain(|_, weak_pool| weak_pool.strong_count() > 0);
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
