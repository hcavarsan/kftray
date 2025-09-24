use log::info;
use sqlx::{
    Row,
    SqlitePool,
};

use crate::models::{
    ShortcutDefinition,
    ShortcutError,
    ShortcutResult,
};

pub struct ShortcutStorage {
    pool: SqlitePool,
}

impl ShortcutStorage {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_shortcut(&self, shortcut: &ShortcutDefinition) -> ShortcutResult<i64> {
        let mut conn = self.pool.acquire().await?;

        let row = sqlx::query(
            "INSERT INTO shortcuts (name, shortcut_key, action_type, action_data, config_id, enabled)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING id"
        )
        .bind(&shortcut.name)
        .bind(&shortcut.shortcut_key)
        .bind(&shortcut.action_type)
        .bind(&shortcut.action_data)
        .bind(shortcut.config_id)
        .bind(shortcut.enabled)
        .fetch_one(&mut *conn)
        .await?;

        let id = row.get("id");
        info!("Created shortcut '{}' with ID: {}", shortcut.name, id);
        Ok(id)
    }

    pub async fn get_shortcut_by_id(&self, id: i64) -> ShortcutResult<Option<ShortcutDefinition>> {
        let mut conn = self.pool.acquire().await?;

        let row = sqlx::query(
            "SELECT id, name, shortcut_key, action_type, action_data, config_id, enabled
             FROM shortcuts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| ShortcutDefinition {
            id: Some(r.get("id")),
            name: r.get("name"),
            shortcut_key: r.get("shortcut_key"),
            action_type: r.get("action_type"),
            action_data: r.get("action_data"),
            config_id: r.get("config_id"),
            enabled: r.get("enabled"),
        }))
    }

    pub async fn get_shortcut_by_name(
        &self, name: &str,
    ) -> ShortcutResult<Option<ShortcutDefinition>> {
        let mut conn = self.pool.acquire().await?;

        let row = sqlx::query(
            "SELECT id, name, shortcut_key, action_type, action_data, config_id, enabled
             FROM shortcuts WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| ShortcutDefinition {
            id: Some(r.get("id")),
            name: r.get("name"),
            shortcut_key: r.get("shortcut_key"),
            action_type: r.get("action_type"),
            action_data: r.get("action_data"),
            config_id: r.get("config_id"),
            enabled: r.get("enabled"),
        }))
    }

    pub async fn get_shortcuts_by_key(
        &self, shortcut_key: &str,
    ) -> ShortcutResult<Vec<ShortcutDefinition>> {
        let mut conn = self.pool.acquire().await?;

        let rows = sqlx::query(
            "SELECT id, name, shortcut_key, action_type, action_data, config_id, enabled
             FROM shortcuts WHERE shortcut_key = ? AND enabled = true",
        )
        .bind(shortcut_key)
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ShortcutDefinition {
                id: Some(r.get("id")),
                name: r.get("name"),
                shortcut_key: r.get("shortcut_key"),
                action_type: r.get("action_type"),
                action_data: r.get("action_data"),
                config_id: r.get("config_id"),
                enabled: r.get("enabled"),
            })
            .collect())
    }

    pub async fn get_all_enabled_shortcuts(&self) -> ShortcutResult<Vec<ShortcutDefinition>> {
        let mut conn = self.pool.acquire().await?;

        let rows = sqlx::query(
            "SELECT id, name, shortcut_key, action_type, action_data, config_id, enabled
             FROM shortcuts WHERE enabled = true
             ORDER BY name",
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ShortcutDefinition {
                id: Some(r.get("id")),
                name: r.get("name"),
                shortcut_key: r.get("shortcut_key"),
                action_type: r.get("action_type"),
                action_data: r.get("action_data"),
                config_id: r.get("config_id"),
                enabled: r.get("enabled"),
            })
            .collect())
    }

    pub async fn get_shortcuts_by_config(
        &self, config_id: i64,
    ) -> ShortcutResult<Vec<ShortcutDefinition>> {
        let mut conn = self.pool.acquire().await?;

        let rows = sqlx::query(
            "SELECT id, name, shortcut_key, action_type, action_data, config_id, enabled
             FROM shortcuts WHERE config_id = ?
             ORDER BY name",
        )
        .bind(config_id)
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ShortcutDefinition {
                id: Some(r.get("id")),
                name: r.get("name"),
                shortcut_key: r.get("shortcut_key"),
                action_type: r.get("action_type"),
                action_data: r.get("action_data"),
                config_id: r.get("config_id"),
                enabled: r.get("enabled"),
            })
            .collect())
    }

    pub async fn update_shortcut(&self, shortcut: &ShortcutDefinition) -> ShortcutResult<()> {
        let id = shortcut.id.ok_or_else(|| {
            ShortcutError::Internal("Shortcut ID is required for update".to_string())
        })?;

        let mut conn = self.pool.acquire().await?;

        let rows_affected = sqlx::query(
            "UPDATE shortcuts
             SET name = ?, shortcut_key = ?, action_type = ?, action_data = ?, config_id = ?, enabled = ?
             WHERE id = ?"
        )
        .bind(&shortcut.name)
        .bind(&shortcut.shortcut_key)
        .bind(&shortcut.action_type)
        .bind(&shortcut.action_data)
        .bind(shortcut.config_id)
        .bind(shortcut.enabled)
        .bind(id)
        .execute(&mut *conn)
        .await?
        .rows_affected();

        if rows_affected == 0 {
            return Err(ShortcutError::NotFound(id.to_string()));
        }

        info!("Updated shortcut ID: {}", id);
        Ok(())
    }

    pub async fn delete_shortcut(&self, id: i64) -> ShortcutResult<()> {
        let mut conn = self.pool.acquire().await?;

        let rows_affected = sqlx::query("DELETE FROM shortcuts WHERE id = ?")
            .bind(id)
            .execute(&mut *conn)
            .await?
            .rows_affected();

        if rows_affected == 0 {
            return Err(ShortcutError::NotFound(id.to_string()));
        }

        info!("Deleted shortcut ID: {}", id);
        Ok(())
    }

    pub async fn delete_shortcuts_by_config(&self, config_id: i64) -> ShortcutResult<u64> {
        let mut conn = self.pool.acquire().await?;

        let rows_affected = sqlx::query("DELETE FROM shortcuts WHERE config_id = ?")
            .bind(config_id)
            .execute(&mut *conn)
            .await?
            .rows_affected();

        info!(
            "Deleted {} shortcuts for config ID: {}",
            rows_affected, config_id
        );
        Ok(rows_affected)
    }

    pub async fn enable_shortcut(&self, id: i64) -> ShortcutResult<()> {
        let mut conn = self.pool.acquire().await?;

        let rows_affected = sqlx::query("UPDATE shortcuts SET enabled = true WHERE id = ?")
            .bind(id)
            .execute(&mut *conn)
            .await?
            .rows_affected();

        if rows_affected == 0 {
            return Err(ShortcutError::NotFound(id.to_string()));
        }

        info!("Enabled shortcut ID: {}", id);
        Ok(())
    }

    pub async fn disable_shortcut(&self, id: i64) -> ShortcutResult<()> {
        let mut conn = self.pool.acquire().await?;

        let rows_affected = sqlx::query("UPDATE shortcuts SET enabled = false WHERE id = ?")
            .bind(id)
            .execute(&mut *conn)
            .await?
            .rows_affected();

        if rows_affected == 0 {
            return Err(ShortcutError::NotFound(id.to_string()));
        }

        info!("Disabled shortcut ID: {}", id);
        Ok(())
    }

    pub async fn shortcut_exists(&self, name: &str) -> ShortcutResult<bool> {
        let mut conn = self.pool.acquire().await?;

        let count: i64 = sqlx::query("SELECT COUNT(*) as count FROM shortcuts WHERE name = ?")
            .bind(name)
            .fetch_one(&mut *conn)
            .await?
            .get("count");

        Ok(count > 0)
    }
}
