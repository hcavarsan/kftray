//! Database migrations
//!
//! This module provides functionality for running database migrations.

use sqlx::Acquire;
use sqlx::{
    Sqlite,
    Transaction,
};
use tracing::info;

use crate::db::Database;
use crate::error::{
    Error,
    Result,
};

const CURRENT_VERSION: i32 = 2;

pub async fn run_migrations(db: &Database) -> Result<()> {
    let mut conn = db.pool.acquire().await?;

    info!("Starting migrations...");

    // Create version table if it doesn't exist
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS version (
            version INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&mut *conn)
    .await?;

    let current_version: i32 = sqlx::query_scalar("SELECT version FROM version")
        .fetch_optional(&mut *conn)
        .await?
        .unwrap_or(0);

    info!("Current database version: {}", current_version);

    if current_version < CURRENT_VERSION {
        info!(
            "Running migrations from version {} to {}",
            current_version, CURRENT_VERSION
        );

        let mut transaction = conn.begin().await?;

        for version in (current_version + 1)..=CURRENT_VERSION {
            info!("Starting migration to version {}", version);
            run_migration(version, &mut transaction).await?;

            // Delete any existing version record first
            sqlx::query("DELETE FROM version")
                .execute(&mut *transaction)
                .await?;

            // Insert new version
            sqlx::query("INSERT INTO version (version) VALUES (?)")
                .bind(version)
                .execute(&mut *transaction)
                .await?;

            info!("Completed migration to version {}", version);
        }

        info!("Committing transaction...");
        transaction.commit().await?;
        info!("Migrations completed successfully");
    } else {
        info!("Database is already at the latest version");
    }

    // Verify final version
    let final_version: i32 = sqlx::query_scalar("SELECT version FROM version")
        .fetch_one(&mut *conn)
        .await?;
    info!("Final database version: {}", final_version);

    Ok(())
}

async fn run_migration(version: i32, transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    info!("Running migration for version {}", version);

    match version {
        1 => migration_v1(transaction).await?,
        2 => migration_v2(transaction).await?,
        _ => return Err(Error::Migration(format!("Unknown version: {}", version))),
    }

    Ok(())
}

async fn migration_v1(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    info!("Creating initial schema");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS configs (
            id INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS config_state (
            id INTEGER PRIMARY KEY,
            config_id INTEGER NOT NULL UNIQUE,  -- Add UNIQUE constraint here
            is_running BOOLEAN NOT NULL DEFAULT FALSE,
            FOREIGN KEY (config_id) REFERENCES configs(id) ON DELETE CASCADE
        );
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

async fn migration_v2(transaction: &mut Transaction<'_, Sqlite>) -> Result<()> {
    info!("Adding triggers for config state management");

    // Drop existing triggers first
    sqlx::query("DROP TRIGGER IF EXISTS after_insert_config")
        .execute(&mut **transaction)
        .await?;

    sqlx::query("DROP TRIGGER IF EXISTS after_delete_config")
        .execute(&mut **transaction)
        .await?;

    // Create new triggers
    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS after_insert_config
        AFTER INSERT ON configs
        BEGIN
            INSERT INTO config_state (config_id, is_running)
            VALUES (NEW.id, FALSE);
        END;
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS after_delete_config
        AFTER DELETE ON configs
        BEGIN
            DELETE FROM config_state WHERE config_id = OLD.id;
        END;
        "#,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use log::LevelFilter;

    use super::*;

    #[tokio::test]
    async fn test_migrations() {
        // Initialize logging for debugging
        env_logger::builder()
            .filter_level(LevelFilter::Info)
            .is_test(true)
            .init();

        info!("Starting migration test");
        let db = Database::new(":memory:".into()).await.unwrap();
        run_migrations(&db).await.unwrap();

        let mut conn = db.pool.acquire().await.unwrap();

        let version: i32 = sqlx::query_scalar("SELECT version FROM version")
            .fetch_one(&mut *conn)
            .await
            .unwrap();

        assert_eq!(version, CURRENT_VERSION, "Migration version mismatch");

        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND (name='configs' OR name='config_state')"
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap();

        info!("Found tables: {:?}", tables);
        assert_eq!(tables.len(), 2, "Missing tables from v1 migration");

        let triggers: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='trigger' AND (name='after_insert_config' OR name='after_delete_config')"
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap();

        info!("Found triggers: {:?}", triggers);
        assert_eq!(triggers.len(), 2, "Missing triggers from v2 migration");
    }
}
