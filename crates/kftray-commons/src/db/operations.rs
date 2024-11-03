//! Database operations
//!
//! This module provides functionality for database operations, including schema
//! management and database operations.

use std::path::PathBuf;

use sqlx::sqlite::{
    SqlitePool,
    SqlitePoolOptions,
};
use sqlx::Acquire;
use sqlx::Row;

use crate::config::Config;
use crate::error::{
    Error,
    Result,
};
use crate::models::state::ConfigState;

#[derive(Clone, Debug)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists with proper permissions
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::db_connection(format!("Failed to create database directory: {}", e)))?;
        }

        // Log the database path for debugging
        tracing::info!("Opening database at: {}", path.display());

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}", path.display()))
            .await
            .map_err(|e| Error::db_connection(format!("Failed to connect to database: {}", e)))?;

        let db = Self { pool };
        db.initialize().await?;
        Ok(db)
    }

    async fn initialize(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;

        sqlx::query(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS configs (
                id INTEGER PRIMARY KEY,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS config_state (
                id INTEGER PRIMARY KEY,
                config_id INTEGER NOT NULL,
                is_running BOOLEAN NOT NULL DEFAULT FALSE,
                FOREIGN KEY (config_id) REFERENCES configs(id) ON DELETE CASCADE,
                UNIQUE(config_id)
            );
            "#,
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    pub async fn save_config(&self, config: &Config) -> Result<i64> {
        let mut conn = self.pool.acquire().await?;
        let data = serde_json::to_string(config)?;

        let id = sqlx::query("INSERT INTO configs (data) VALUES (?) RETURNING id")
            .bind(data)
            .fetch_one(&mut *conn)
            .await?
            .get(0);

        Ok(id)
    }

    pub async fn get_config(&self, id: i64) -> Result<Config> {
        let mut conn = self.pool.acquire().await?;
        let record = sqlx::query("SELECT data FROM configs WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;

        let data: String = record.get("data");
        let mut config: Config = serde_json::from_str(&data)?;
        config.id = Some(id);

        Ok(config)
    }

    pub async fn get_all_configs(&self) -> Result<Vec<Config>> {
        let mut conn = self.pool.acquire().await?;
        let rows = sqlx::query("SELECT id, data FROM configs")
            .fetch_all(&mut *conn)
            .await?;

        let mut configs = Vec::with_capacity(rows.len());
        for row in rows {
            let id: i64 = row.get("id");
            let data: String = row.get("data");
            let mut config: Config = serde_json::from_str(&data)?;
            config.id = Some(id);
            configs.push(config);
        }

        Ok(configs)
    }

    pub async fn update_config(&self, config: &Config) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let data = serde_json::to_string(config)?;

        sqlx::query("UPDATE configs SET data = ? WHERE id = ?")
            .bind(data)
            .bind(
                config
                    .id
                    .ok_or_else(|| Error::validation("Config ID is required"))?,
            )
            .execute(&mut *conn)
            .await?;

        Ok(())
    }

    pub async fn delete_config(&self, id: i64) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("DELETE FROM configs WHERE id = ?")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    pub async fn delete_configs(&self, ids: &[i64]) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;

        for &id in ids {
            sqlx::query("DELETE FROM configs WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn clear_all_configs(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("DELETE FROM configs")
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    pub async fn get_config_state(&self, config_id: i64) -> Result<ConfigState> {
        let mut conn = self.pool.acquire().await?;
        let record = sqlx::query("SELECT id, is_running FROM config_state WHERE config_id = ?")
            .bind(config_id)
            .fetch_one(&mut *conn)
            .await?;

        Ok(ConfigState {
            id: Some(record.get("id")),
            config_id,
            is_running: record.get("is_running"),
        })
    }

    pub async fn update_config_state(&self, state: &ConfigState) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query(
            r#"
            INSERT INTO config_state (config_id, is_running)
            VALUES (?, ?)
            ON CONFLICT(config_id) DO UPDATE SET
            is_running = excluded.is_running
            WHERE config_id = excluded.config_id
            "#,
        )
        .bind(state.config_id)
        .bind(state.is_running)
        .execute(&mut *conn)
        .await?;

        Ok(())
    }

    pub async fn get_all_config_states(&self) -> Result<Vec<ConfigState>> {
        let mut conn = self.pool.acquire().await?;
        let rows = sqlx::query("SELECT id, config_id, is_running FROM config_state")
            .fetch_all(&mut *conn)
            .await?;

        let states = rows
            .into_iter()
            .map(|row| ConfigState {
                id: Some(row.get("id")),
                config_id: row.get("config_id"),
                is_running: row.get("is_running"),
            })
            .collect();

        Ok(states)
    }

    pub async fn is_connected(&self) -> Result<bool> {
        self.pool.acquire().await.map(|_| true).map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_operations() {
        let db = Database::new(":memory:".into()).await.unwrap();

        // Test config operations
        let config = Config::default();

        let id = db.save_config(&config).await.unwrap();
        let saved_config = db.get_config(id).await.unwrap();
        assert_eq!(saved_config.id, Some(id));

        // Test state operations
        let state = ConfigState {
            id: None,
            config_id: id,
            is_running: true,
        };

        db.update_config_state(&state).await.unwrap();
        let saved_state = db.get_config_state(id).await.unwrap();
        assert_eq!(saved_state.config_id, id);
        assert!(saved_state.is_running);
    }
}
