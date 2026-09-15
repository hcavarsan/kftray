pub mod models;
pub mod utils;

pub use models::*;
pub use utils::*;

pub mod test_utils {
    use lazy_static::lazy_static;
    use tokio::sync::Mutex;

    lazy_static! {
        pub static ref MEMORY_MODE_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    /// Sets or removes an environment variable for the duration of a test,
    /// restoring the previous value (or absence) on drop, including on
    /// panic (unwinding, not `abort`).
    pub struct EnvVarGuard {
        key: String,
        original_value: Option<String>,
    }

    impl EnvVarGuard {
        pub fn set(key: &str, value: &str) -> Self {
            let key = key.to_string();
            let original_value = std::env::var(&key).ok();
            unsafe { std::env::set_var(&key, value) };
            EnvVarGuard {
                key,
                original_value,
            }
        }

        pub fn remove(key: &str) -> Self {
            let key = key.to_string();
            let original_value = std::env::var(&key).ok();
            unsafe { std::env::remove_var(&key) };
            EnvVarGuard {
                key,
                original_value,
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
