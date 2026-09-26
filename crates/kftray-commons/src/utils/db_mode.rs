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

static MEMORY_DB_POOL: LazyLock<Mutex<Option<Arc<SqlitePool>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Serializes creation of the memory pool so concurrent first callers share
/// one pool instead of each creating one and the last write winning.
static MEMORY_DB_INIT: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

type PoolSlot = Mutex<Option<Arc<SqlitePool>>>;

async fn get_or_init_pool<F, Fut>(
    slot: &PoolSlot, init_lock: &tokio::sync::Mutex<()>, init: F,
) -> Result<Arc<SqlitePool>, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Arc<SqlitePool>, String>>,
{
    if let Some(pool) = slot.lock().unwrap().clone() {
        return Ok(pool);
    }
    let _init = init_lock.lock().await;
    if let Some(pool) = slot.lock().unwrap().clone() {
        return Ok(pool);
    }
    let pool = init().await?;
    *slot.lock().unwrap() = Some(pool.clone());
    Ok(pool)
}

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
            DatabaseMode::Memory => {
                get_or_init_pool(&MEMORY_DB_POOL, &MEMORY_DB_INIT, create_memory_pool).await?
            }
        };
        Ok(DatabaseContext { pool, mode })
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
    async fn test_concurrent_first_callers_share_one_pool() {
        use std::sync::atomic::{
            AtomicUsize,
            Ordering,
        };

        let slot = PoolSlot::default();
        let init_lock = tokio::sync::Mutex::new(());
        let inits = AtomicUsize::new(0);
        let init = || async {
            inits.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            SqlitePool::connect("sqlite::memory:")
                .await
                .map(Arc::new)
                .map_err(|e| e.to_string())
        };

        let pools =
            futures::future::join_all((0..8).map(|_| get_or_init_pool(&slot, &init_lock, init)))
                .await;
        let pools: Vec<_> = pools.into_iter().map(Result::unwrap).collect();

        assert_eq!(inits.load(Ordering::SeqCst), 1);
        assert!(pools.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])));
    }

    #[tokio::test]
    async fn test_database_context_file() {
        let context = DatabaseManager::get_context(DatabaseMode::File).await;
        if let Ok(ctx) = context {
            assert_eq!(ctx.mode, DatabaseMode::File);
        }
    }
}
