pub mod models;
pub mod utils;

pub use models::*;
pub use utils::*;

pub mod test_utils {
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::sync::MutexGuard;

    use lazy_static::lazy_static;
    use tokio::sync::Mutex as AsyncMutex;

    lazy_static! {
        pub static ref MEMORY_MODE_TEST_MUTEX: AsyncMutex<()> = AsyncMutex::new(());
    }

    /// Serializes environment-variable mutation across tests: `std::env` is
    /// global to the process, so two tests mutating it on different threads
    /// must not interleave.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    thread_local! {
        // Depth of `EnvVarGuard`s already holding `ENV_MUTEX` on this
        // thread. A test commonly builds up several guards at once, one per
        // variable; a plain, non-reentrant lock would deadlock the second
        // guard against the first held by the same thread.
        static ENV_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    /// Holds `ENV_MUTEX` for as long as at least one `EnvVarGuard` on this
    /// thread is alive, released only when the outermost one drops.
    ///
    /// Relies on guards being dropped in the reverse order they were
    /// created, which is what happens for ordinary stack-local bindings:
    /// the guard that actually owns the lock is the first one created, so
    /// it is also the last one dropped.
    enum EnvLock {
        Owned { _guard: MutexGuard<'static, ()> },
        Nested,
    }

    impl EnvLock {
        fn acquire() -> Self {
            let depth = ENV_LOCK_DEPTH.with(Cell::get);
            ENV_LOCK_DEPTH.with(|cell| cell.set(depth + 1));
            if depth == 0 {
                EnvLock::Owned {
                    _guard: ENV_MUTEX.lock().unwrap_or_else(|error| error.into_inner()),
                }
            } else {
                EnvLock::Nested
            }
        }
    }

    impl Drop for EnvLock {
        fn drop(&mut self) {
            let depth = ENV_LOCK_DEPTH.with(Cell::get);
            if let EnvLock::Owned { .. } = self {
                debug_assert_eq!(
                    depth, 1,
                    "EnvVarGuard dropped out of order: the guard holding ENV_MUTEX must be the \
                     last one dropped, which requires guards to be plain stack bindings"
                );
            }
            ENV_LOCK_DEPTH.with(|cell| cell.set(depth - 1));
        }
    }

    /// Sets or removes an environment variable for the duration of a test,
    /// restoring the previous value (or absence) on drop, including on
    /// panic (unwinding, not `abort`). Also holds a process-wide lock (see
    /// [`EnvLock`]) for its lifetime, so no other thread observes the
    /// mutation mid-test.
    pub struct EnvVarGuard {
        key: String,
        original_value: Option<String>,
        _lock: EnvLock,
    }

    impl EnvVarGuard {
        pub fn set(key: &str, value: &str) -> Self {
            let _lock = EnvLock::acquire();
            let key = key.to_string();
            let original_value = std::env::var(&key).ok();
            unsafe { std::env::set_var(&key, value) };
            EnvVarGuard {
                key,
                original_value,
                _lock,
            }
        }

        pub fn remove(key: &str) -> Self {
            let _lock = EnvLock::acquire();
            let key = key.to_string();
            let original_value = std::env::var(&key).ok();
            unsafe { std::env::remove_var(&key) };
            EnvVarGuard {
                key,
                original_value,
                _lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(val) => unsafe { std::env::set_var(&self.key, val) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }
}
