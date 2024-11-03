//! KFtray Commons Library
//!
//! This library provides core functionality for managing Kubernetes port
//! forwarding configurations, state management, and utility functions used
//! across the KFtray application ecosystem.
//!
//! # Main Components
//!
//! - `config`: Configuration management and validation
//! - `core`: Core functionality for Kubernetes and GitHub interactions
//! - `db`: Database operations and migrations
//! - `error`: Error types and handling
//! - `models`: Data models and types
//! - `utils`: Utility functions for logging, paths, state management, etc.
//!
//! # Example
//!
//! ```no_run
//! use kftray_commons::prelude::*;
//! use kftray_commons::{init, Result};
//!
//! async fn example() -> Result<()> {
//!     // Initialize database and state manager
//!     let (db, state_manager) = init().await?;
//!
//!     // Create and validate a configuration
//!     let config = Config::builder()
//!         .namespace("default")
//!         .protocol("TCP")
//!         .local_port(8080)
//!         .build()?;
//!
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod core;
pub mod db;
pub mod error;
pub mod models;
pub mod utils;

use db::operations::Database;
pub use error::Result;
use tracing::error;
use utils::state::StateManager;

/// Re-export common types and functionality
pub mod prelude {
    pub use super::config::Config;
    pub use super::core::*;
    pub use super::error::{
        Error,
        Result,
    };
    pub use super::models::*;
}

/// Initialize the database and state manager
pub async fn init() -> Result<(Database, StateManager)> {
    let db_path = utils::paths::get_db_path().await?;
    let database = Database::new(db_path).await?;

    // Run migrations
    db::migrations::run_migrations(&database).await?;

    let state_manager = StateManager::new(database.clone()).await?;

    Ok((database, state_manager))
}

/// Check and manage port configurations
pub async fn check_and_manage_ports(
    database: &Database, state_manager: &StateManager,
) -> Result<()> {
    let states = state_manager.get_all_states().await?;

    for state in states {
        if state.is_running {
            if let Ok(config) = database.get_config(state.config_id).await {
                if let Err(err) =
                    utils::validation::check_and_manage_port(&config, state_manager).await
                {
                    error!(
                        "Error checking state for config {}: {}",
                        state.config_id, err
                    );
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use config::Config;

    use super::*;

    #[tokio::test]
    async fn test_initialization() {
        let database = Database::new(":memory:".into()).await.unwrap();
        let _state_manager = StateManager::new(database.clone()).await.unwrap();

        assert!(database.is_connected().await.unwrap());
    }

    #[tokio::test]
    async fn test_port_management() {
        // Use in-memory database
        let database = Database::new(":memory:".into()).await.unwrap();

        // Run migrations before creating state manager
        db::migrations::run_migrations(&database).await.unwrap();

        let state_manager = StateManager::new(database.clone()).await.unwrap();

        // Create test config with valid values
        let config = Config::builder()
            .namespace("test")
            .protocol("TCP")
            .local_port(8080)
            .build()
            .unwrap();

        let id = database.save_config(&config).await.unwrap();
        state_manager.update_state(id, true).await.unwrap();

        check_and_manage_ports(&database, &state_manager)
            .await
            .unwrap();
    }
}
