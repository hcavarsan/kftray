//! Database operations and migrations
//!
//! This module provides functionality for database operations and migrations,
//! including schema management and database operations.

pub mod migrations;
pub mod operations;

// Re-export the Database struct and its operations
pub use operations::Database;

// Re-export get_db_path for database initialization
pub use crate::utils::paths::get_db_path;
