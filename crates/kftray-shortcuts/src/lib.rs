pub mod actions;
pub mod manager;
pub mod models;
pub mod parser;
pub mod platforms;
pub mod storage;

pub use actions::{
    ActionHandler,
    ActionRegistry,
    create_default_registry,
};
pub use manager::ShortcutManager;
pub use models::{
    ActionContext,
    ShortcutDefinition,
    ShortcutError,
    ShortcutPlatform,
    ShortcutResult,
};
pub use parser::{
    ParsedShortcut,
    ShortcutParser,
};
pub use storage::ShortcutStorage;

pub async fn create_manager(pool: sqlx::SqlitePool) -> ShortcutResult<ShortcutManager> {
    ShortcutManager::new(pool).await
}

pub async fn create_manager_with_registry(
    pool: sqlx::SqlitePool, registry: ActionRegistry,
) -> ShortcutResult<ShortcutManager> {
    ShortcutManager::with_custom_registry(pool, registry).await
}
