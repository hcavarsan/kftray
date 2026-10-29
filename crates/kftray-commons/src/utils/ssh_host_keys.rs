use sqlx::{
    Row,
    SqlitePool,
};

use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};

pub const SSH_HOST_KEYS_DDL: &str = "CREATE TABLE IF NOT EXISTS ssh_host_keys (host_alias TEXT \
                                     PRIMARY KEY, key_type TEXT NOT NULL, key_b64 TEXT NOT NULL, \
                                     first_seen INTEGER NOT NULL)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredHostKey {
    pub key_type: String,
    pub key_b64: String,
    pub first_seen: i64,
}

async fn get_with_pool(alias: &str, pool: &SqlitePool) -> Result<Option<StoredHostKey>, String> {
    let row = sqlx::query(
        "SELECT key_type, key_b64, first_seen FROM ssh_host_keys WHERE host_alias = ?1",
    )
    .bind(alias)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    row.map(|row| {
        Ok(StoredHostKey {
            key_type: row.try_get("key_type").map_err(|e| e.to_string())?,
            key_b64: row.try_get("key_b64").map_err(|e| e.to_string())?,
            first_seen: row.try_get("first_seen").map_err(|e| e.to_string())?,
        })
    })
    .transpose()
}

async fn insert_if_absent_with_pool(
    alias: &str, key_type: &str, key_b64: &str, pool: &SqlitePool,
) -> Result<StoredHostKey, String> {
    let first_seen = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    sqlx::query(
        "INSERT INTO ssh_host_keys (host_alias, key_type, key_b64, first_seen) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(host_alias) DO NOTHING",
    )
    .bind(alias)
    .bind(key_type)
    .bind(key_b64)
    .bind(first_seen)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    get_with_pool(alias, pool)
        .await?
        .ok_or_else(|| format!("Host key for {alias} vanished after insert"))
}

async fn delete_with_pool(alias: &str, pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("DELETE FROM ssh_host_keys WHERE host_alias = ?1")
        .bind(alias)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_ssh_host_key_with_mode(
    alias: &str, mode: DatabaseMode,
) -> Result<Option<StoredHostKey>, String> {
    let context = DatabaseManager::get_context(mode).await?;
    get_with_pool(alias, &context.pool).await
}

/// Stores the key unless the alias already has one, then returns the stored row.
pub async fn insert_ssh_host_key_if_absent_with_mode(
    alias: &str, key_type: &str, key_b64: &str, mode: DatabaseMode,
) -> Result<StoredHostKey, String> {
    let context = DatabaseManager::get_context(mode).await?;
    insert_if_absent_with_pool(alias, key_type, key_b64, &context.pool).await
}

pub async fn delete_ssh_host_key_with_mode(alias: &str, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    delete_with_pool(alias, &context.pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_if_absent_should_keep_first_key_when_inserted_twice() {
        let _db = crate::test_utils::test_db().await;
        let mode = DatabaseMode::Memory;
        let alias = "gcp:compute.1";

        assert_eq!(get_ssh_host_key_with_mode(alias, mode).await.unwrap(), None);

        let first = insert_ssh_host_key_if_absent_with_mode(alias, "ssh-ed25519", "AAAA", mode)
            .await
            .unwrap();
        let second = insert_ssh_host_key_if_absent_with_mode(alias, "ssh-rsa", "BBBB", mode)
            .await
            .unwrap();

        assert_eq!(first.key_b64, "AAAA");
        assert_eq!(second, first);
        assert_eq!(
            get_ssh_host_key_with_mode(alias, mode).await.unwrap(),
            Some(first)
        );
    }

    #[tokio::test]
    async fn delete_should_allow_a_new_key_when_host_key_was_forgotten() {
        let _db = crate::test_utils::test_db().await;
        let mode = DatabaseMode::Memory;
        let alias = "gcp:compute.2";

        insert_ssh_host_key_if_absent_with_mode(alias, "ssh-ed25519", "AAAA", mode)
            .await
            .unwrap();
        delete_ssh_host_key_with_mode(alias, mode).await.unwrap();
        let stored = insert_ssh_host_key_if_absent_with_mode(alias, "ssh-rsa", "BBBB", mode)
            .await
            .unwrap();

        assert_eq!(stored.key_b64, "BBBB");
    }
}
