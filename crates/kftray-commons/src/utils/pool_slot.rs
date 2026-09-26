use std::future::Future;
use std::sync::{
    Arc,
    Mutex,
};

use sqlx::SqlitePool;

/// A lazily created pool that tests can swap or clear, so each test gets a
/// pool bound to its own runtime. sqlx hands connections back to the pool by
/// spawning on the current runtime, and a pool shared across test runtimes
/// leaks those connections whenever a runtime shuts down first.
pub(crate) struct PoolSlot {
    pool: Mutex<Option<Arc<SqlitePool>>>,
    init: tokio::sync::Mutex<()>,
}

impl PoolSlot {
    pub(crate) const fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            init: tokio::sync::Mutex::const_new(()),
        }
    }

    fn current(&self) -> Option<Arc<SqlitePool>> {
        self.pool.lock().unwrap().clone()
    }

    /// Returns the pool, creating it once even when several callers race on
    /// the first use.
    pub(crate) async fn get_or_init<F, Fut>(&self, init: F) -> Result<Arc<SqlitePool>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Arc<SqlitePool>, String>>,
    {
        if let Some(pool) = self.current() {
            return Ok(pool);
        }
        let _init = self.init.lock().await;
        if let Some(pool) = self.current() {
            return Ok(pool);
        }
        let pool = init().await?;
        self.set(pool.clone());
        Ok(pool)
    }

    pub(crate) fn set(&self, pool: Arc<SqlitePool>) {
        *self.pool.lock().unwrap() = Some(pool);
    }

    pub(crate) fn clear(&self) {
        self.pool.lock().unwrap().take();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    use super::*;

    #[tokio::test]
    async fn concurrent_first_callers_share_one_pool() {
        let _db = crate::test_utils::test_db().await;
        let slot = PoolSlot::new();
        let inits = AtomicUsize::new(0);
        let init = || async {
            inits.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            SqlitePool::connect("sqlite::memory:")
                .await
                .map(Arc::new)
                .map_err(|e| e.to_string())
        };

        let pools = futures::future::join_all((0..8).map(|_| slot.get_or_init(init))).await;
        let pools: Vec<_> = pools.into_iter().map(Result::unwrap).collect();

        assert_eq!(inits.load(Ordering::SeqCst), 1);
        assert!(pools.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])));
    }

    #[tokio::test]
    async fn clear_drops_the_pool() {
        let _db = crate::test_utils::test_db().await;
        let slot = PoolSlot::new();
        let pool = Arc::new(SqlitePool::connect("sqlite::memory:").await.unwrap());
        slot.set(pool.clone());
        slot.clear();

        let next = slot
            .get_or_init(|| async {
                SqlitePool::connect("sqlite::memory:")
                    .await
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&pool, &next));
    }
}
