use futures::stream::StreamExt;
use log::{
    error,
    info,
};
use portpicker::pick_unused_port;
use serde_json::json;
use sqlx::{
    Row,
    SqlitePool,
};

use crate::db::{
    create_db_table,
    get_db_pool,
};
use crate::hostsfile::HostsFile;
use crate::migration::migrate_configs;
use crate::models::config_model::Config;
use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};
use crate::utils::error::DbError;

pub async fn delete_config_with_pool(id: i64, pool: &SqlitePool) -> Result<(), DbError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("DELETE FROM configs WHERE id = ?1")
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(|e| DbError::QueryFailed(format!("Failed to delete config: {e}")))?;
    Ok(())
}

pub async fn delete_config(id: i64) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    delete_config_with_pool(id, &pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_configs_with_pool(ids: Vec<i64>, pool: &SqlitePool) -> Result<(), DbError> {
    let mut transaction = pool.begin().await?;
    for id in ids {
        sqlx::query("DELETE FROM configs WHERE id = ?1")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| {
                DbError::QueryFailed(format!("Failed to delete config with id {id}: {e}"))
            })?;
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn delete_configs(ids: Vec<i64>) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    delete_configs_with_pool(ids, &pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_all_configs_with_pool(pool: &SqlitePool) -> Result<(), DbError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("DELETE FROM configs")
        .execute(&mut *conn)
        .await
        .map_err(|e| DbError::QueryFailed(format!("Failed to delete all configs: {e}")))?;
    Ok(())
}

pub async fn delete_all_configs() -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    delete_all_configs_with_pool(&pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn insert_config_with_pool(config: Config, pool: &SqlitePool) -> Result<(), String> {
    let config = prepare_config(config);
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    create_db_table(pool).await.map_err(|e| e.to_string())?;

    // Validate that file mode won't conflict with memory mode ID range
    let memory_id_start = std::env::var("KFTRAY_MEMORY_ID_START")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(MEMORY_ID_START);

    let next_id_row =
        sqlx::query("SELECT COALESCE(MAX(id), 0) + 1 as next_id FROM configs WHERE id < ?")
            .bind(memory_id_start)
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;

    let next_id: i64 = next_id_row.try_get("next_id").map_err(|e| e.to_string())?;

    if next_id >= memory_id_start {
        return Err(format!(
            "ID conflict detected: next file mode ID ({next_id}) would exceed memory mode start ({memory_id_start})"
        ));
    }

    let data = json!(config).to_string();
    let result = sqlx::query("INSERT INTO configs (data) VALUES (?1)")
        .bind(data)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    let inserted_id = result.last_insert_rowid();
    sync_http_logs_config_from_config(&config, inserted_id, pool).await?;

    Ok(())
}

pub(crate) async fn insert_config_with_pool_and_mode(
    config: Config, pool: &SqlitePool, mode: DatabaseMode,
) -> Result<(), String> {
    match mode {
        DatabaseMode::Memory => {
            let config = prepare_config(config);
            let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
            create_db_table(pool).await.map_err(|e| e.to_string())?;

            let next_id = get_next_memory_id(pool).await?;
            let data = json!(config).to_string();

            sqlx::query("INSERT INTO configs (id, data) VALUES (?1, ?2)")
                .bind(next_id)
                .bind(data)
                .execute(&mut *conn)
                .await
                .map_err(|e| e.to_string())?;

            sync_http_logs_config_from_config(&config, next_id, pool).await?;
            Ok(())
        }
        DatabaseMode::File => insert_config_with_pool(config, pool).await,
    }
}

const MEMORY_ID_START: i64 = 100000;

async fn get_next_memory_id(pool: &SqlitePool) -> Result<i64, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let memory_id_start = std::env::var("KFTRAY_MEMORY_ID_START")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(MEMORY_ID_START);

    // Validate that memory ID range doesn't conflict with existing file-based IDs
    let file_max_row =
        sqlx::query("SELECT COALESCE(MAX(id), 0) as max_id FROM configs WHERE id < ?")
            .bind(memory_id_start)
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;

    let file_max_id: i64 = file_max_row.try_get("max_id").map_err(|e| e.to_string())?;

    if file_max_id >= memory_id_start {
        return Err(format!(
            "ID conflict detected: file mode max ID ({file_max_id}) exceeds memory mode start ({memory_id_start})"
        ));
    }

    let row = sqlx::query("SELECT COALESCE(MAX(id), ?) as max_id FROM configs WHERE id >= ?")
        .bind(memory_id_start - 1)
        .bind(memory_id_start)
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    let max_id: i64 = row.try_get("max_id").map_err(|e| e.to_string())?;

    max_id
        .checked_add(1)
        .ok_or_else(|| "ID overflow: maximum ID value reached".to_string())
}

pub async fn insert_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    insert_config_with_pool(config, &pool).await
}

pub async fn read_configs_with_pool(pool: &SqlitePool) -> Result<Vec<Config>, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT id, data FROM configs")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    let config_results: Vec<Result<Config, String>> = futures::stream::iter(rows.into_iter())
        .map(|row| {
            let id: Result<i64, String> = row.try_get("id").map_err(|e| e.to_string());
            let data: Result<String, String> = row.try_get("data").map_err(|e| e.to_string());
            async move {
                let id = id?;
                let data = data?;
                let mut config = serde_json::from_str::<Config>(&data)
                    .map_err(|_| "Failed to decode config".to_string())?;
                config.id = Some(id);
                Ok(config)
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<Result<Config, String>>>()
        .await;

    let mut configs = Vec::new();
    for result in config_results {
        match result {
            Ok(config) => configs.push(config),
            Err(e) => return Err(e),
        }
    }
    Ok(configs)
}

pub async fn read_configs() -> Result<Vec<Config>, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    read_configs_with_pool(&pool).await
}

pub(crate) async fn clean_all_custom_hosts_entries_with_pool(
    pool: &SqlitePool,
) -> Result<(), String> {
    clean_all_custom_hosts_entries_with_pool_and_path(pool, None).await
}

async fn clean_all_custom_hosts_entries_with_pool_and_path(
    pool: &SqlitePool, custom_hosts_path: Option<&std::path::Path>,
) -> Result<(), String> {
    let configs = read_configs_with_pool(pool)
        .await
        .map_err(|e| e.to_string())?;
    for config in configs {
        let hostfile_comment = format!(
            "kftray custom host for {} - {}",
            config.service.unwrap_or_default(),
            config.id.unwrap_or_default()
        );
        let hosts_file = HostsFile::new(&hostfile_comment);
        let result = match custom_hosts_path {
            Some(path) => hosts_file.write_to(path),
            None => hosts_file.write(),
        };
        result
            .map_err(|e| format!("Failed to write to the hostfile for {hostfile_comment}: {e}"))?;
    }
    Ok(())
}

pub async fn clean_all_custom_hosts_entries() -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    clean_all_custom_hosts_entries_with_pool(&pool).await
}

pub async fn get_configs() -> Result<Vec<Config>, String> {
    read_configs().await
}

pub(crate) async fn get_config_with_pool(id: i64, pool: &SqlitePool) -> Result<Config, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT id, data FROM configs WHERE id = ?1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    match row {
        Some(row) => {
            let id: i64 = row.try_get("id").map_err(|e| e.to_string())?;
            let data: String = row.try_get("data").map_err(|e| e.to_string())?;
            let mut config: Config =
                serde_json::from_str(&data).map_err(|e| format!("Failed to parse config: {e}"))?;
            config.id = Some(id);
            Ok(config)
        }
        None => Err(format!("No config found with id: {id}")),
    }
}

pub async fn get_config(id: i64) -> Result<Config, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    get_config_with_pool(id, &pool).await
}

pub(crate) async fn update_config_with_pool(
    config: Config, pool: &SqlitePool,
) -> Result<(), String> {
    let config = prepare_config(config);
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let data = json!(config).to_string();
    sqlx::query("UPDATE configs SET data = ?1 WHERE id = ?2")
        .bind(data)
        .bind(config.id.unwrap())
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(config_id) = config.id {
        sync_http_logs_config_from_config(&config, config_id, pool).await?;
    }

    Ok(())
}

pub async fn update_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    update_config_with_pool(config, &pool).await
}

pub(crate) async fn export_configs_with_pool(pool: &SqlitePool) -> Result<String, String> {
    let configs: Vec<Config> = read_configs_with_pool(pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|c| c.prepare_for_export())
        .collect();

    serde_json::to_string_pretty(&configs).map_err(|e| e.to_string())
}

pub async fn export_configs() -> Result<String, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    export_configs_with_pool(&pool).await
}

fn validate_imported_config(config: &Config) -> Result<(), String> {
    if config.namespace.is_empty() {
        return Err("Namespace is required and cannot be empty".to_string());
    }

    match config.workload_type.as_deref() {
        Some("service") => {
            if config.service.is_none() || config.service.as_ref().unwrap().is_empty() {
                return Err("Service name is required for service workload type".to_string());
            }
        }
        Some("pod") => {
            if config.target.is_none() || config.target.as_ref().unwrap().is_empty() {
                return Err(
                    "Target (pod label selector) is required for pod workload type".to_string(),
                );
            }
        }
        Some("proxy") => {
            if config.remote_address.is_none() || config.remote_address.as_ref().unwrap().is_empty()
            {
                return Err("Remote address is required for proxy workload type".to_string());
            }
        }
        Some(workload_type) => {
            return Err(format!(
                "Invalid workload type: {workload_type}. Must be 'service', 'pod', or 'proxy'"
            ));
        }
        None => {
            return Err("Workload type is required".to_string());
        }
    }

    if let Some(max_file_size) = config.http_logs_max_file_size {
        if max_file_size == 0 {
            return Err("HTTP logs max file size must be greater than 0".to_string());
        }
        if max_file_size > 100 * 1024 * 1024 {
            return Err("HTTP logs max file size cannot exceed 100MB".to_string());
        }
    }

    if let Some(retention_days) = config.http_logs_retention_days {
        if retention_days == 0 {
            return Err("HTTP logs retention days must be greater than 0".to_string());
        }
        if retention_days > 365 {
            return Err("HTTP logs retention days cannot exceed 365 days".to_string());
        }
    }

    Ok(())
}

fn configs_match_identity(existing: &Config, incoming: &Config) -> bool {
    if existing.context != incoming.context
        || existing.namespace != incoming.namespace
        || existing.workload_type != incoming.workload_type
        || existing.protocol != incoming.protocol
    {
        return false;
    }

    match existing.workload_type.as_deref() {
        Some("service") => existing.service == incoming.service,
        Some("pod") => existing.target == incoming.target,
        Some("proxy") => existing.remote_address == incoming.remote_address,
        _ => {
            existing.service == incoming.service
                && existing.target == incoming.target
                && existing.remote_address == incoming.remote_address
        }
    }
}

fn configs_are_identical(existing: &Config, incoming: &Config) -> bool {
    let mut existing_clone = existing.clone();
    let mut incoming_clone = incoming.clone();

    existing_clone.id = None;
    incoming_clone.id = None;

    existing_clone == incoming_clone
}

async fn merge_config_with_existing(
    config: Config, existing_configs: &[Config], pool: &SqlitePool,
) -> Result<(), String> {
    if let Some(existing) = existing_configs
        .iter()
        .find(|c| configs_match_identity(c, &config))
    {
        info!(
            "Found matching config ID={}, checking if update needed",
            existing.id.unwrap_or(-1)
        );
        if configs_are_identical(existing, &config) {
            info!("Config is identical, skipping");
            return Ok(());
        }

        info!("Config has changes, updating");
        let mut updated_config = config;
        updated_config.id = existing.id;

        if updated_config.alias.is_none() || updated_config.alias.as_deref() == Some("") {
            updated_config.alias = existing.alias.clone();
        }

        if updated_config.local_port.is_none() || updated_config.local_port == Some(0) {
            updated_config.local_port = existing.local_port;
        }

        update_config_with_pool(updated_config, pool).await?;
    } else {
        info!("No matching config found, inserting new config");
        insert_config_with_pool(config, pool).await?;
    }

    Ok(())
}

pub(crate) async fn import_configs_with_pool(
    json: String, pool: &SqlitePool,
) -> Result<(), String> {
    let configs = parse_config_json(&json)?;

    let existing_configs = read_configs_with_pool(pool).await?;

    for config in configs {
        validate_imported_config(&config).map_err(|e| format!("Invalid config: {e}"))?;
        merge_config_with_existing(config, &existing_configs, pool)
            .await
            .map_err(|e| format!("Failed to merge config: {e}"))?;
    }

    if let Err(e) = migrate_configs(Some(pool)).await {
        return Err(format!("Error migrating configs: {e}"));
    }

    Ok(())
}

fn parse_config_json(json: &str) -> Result<Vec<Config>, String> {
    match serde_json::from_str(json) {
        Ok(configs) => Ok(configs),
        Err(e) => {
            info!("Failed to parse JSON as Vec<Config>: {e}. Trying as single Config.");
            let config = serde_json::from_str::<Config>(json)
                .map_err(|e| format!("Failed to parse config: {e}"))?;
            Ok(vec![config])
        }
    }
}

async fn merge_config_with_existing_and_mode(
    config: Config, existing_configs: &[Config], pool: &SqlitePool, mode: DatabaseMode,
) -> Result<(), String> {
    if let Some(existing) = existing_configs
        .iter()
        .find(|c| configs_match_identity(c, &config))
    {
        info!(
            "Found matching config ID={}, checking if update needed",
            existing.id.unwrap_or(-1)
        );
        if configs_are_identical(existing, &config) {
            info!("Config is identical, skipping");
            return Ok(());
        }

        info!("Config has changes, updating");
        let mut updated_config = config;
        updated_config.id = existing.id;

        if updated_config.alias.is_none() || updated_config.alias.as_deref() == Some("") {
            updated_config.alias = existing.alias.clone();
        }

        if updated_config.local_port.is_none() || updated_config.local_port == Some(0) {
            updated_config.local_port = existing.local_port;
        }

        update_config_with_pool(updated_config, pool).await?;
    } else {
        info!("No matching config found, inserting new config");
        insert_config_with_pool_and_mode(config, pool, mode).await?;
    }

    Ok(())
}

pub(crate) async fn import_configs_with_pool_and_mode(
    json: String, pool: &SqlitePool, mode: DatabaseMode,
) -> Result<(), String> {
    let configs = parse_config_json(&json)?;

    let existing_configs = read_configs_with_pool(pool).await?;

    for config in configs {
        validate_imported_config(&config).map_err(|e| format!("Invalid config: {e}"))?;
        merge_config_with_existing_and_mode(config, &existing_configs, pool, mode)
            .await
            .map_err(|e| format!("Failed to merge config: {e}"))?;
    }

    if let Err(e) = migrate_configs(Some(pool)).await {
        return Err(format!("Error migrating configs: {e}"));
    }

    Ok(())
}

pub async fn import_configs(json: String) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    import_configs_with_pool(json, &pool).await
}

pub async fn delete_config_with_mode(id: i64, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    delete_config_with_pool(id, &context.pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_configs_with_mode(ids: Vec<i64>, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    delete_configs_with_pool(ids, &context.pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_all_configs_with_mode(mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    delete_all_configs_with_pool(&context.pool)
        .await
        .map_err(|e| e.to_string())
}

pub async fn insert_config_with_mode(config: Config, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    insert_config_with_pool_and_mode(config, &context.pool, mode).await
}

pub async fn read_configs_with_mode(mode: DatabaseMode) -> Result<Vec<Config>, String> {
    let context = DatabaseManager::get_context(mode).await?;
    read_configs_with_pool(&context.pool).await
}

pub async fn get_config_with_mode(id: i64, mode: DatabaseMode) -> Result<Config, String> {
    let context = DatabaseManager::get_context(mode).await?;
    get_config_with_pool(id, &context.pool).await
}

pub async fn update_config_with_mode(config: Config, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    update_config_with_pool(config, &context.pool).await
}

pub async fn export_configs_with_mode(mode: DatabaseMode) -> Result<String, String> {
    let context = DatabaseManager::get_context(mode).await?;
    export_configs_with_pool(&context.pool).await
}

pub async fn import_configs_with_mode(json: String, mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    import_configs_with_pool_and_mode(json, &context.pool, mode).await
}

pub async fn get_configs_with_mode(mode: DatabaseMode) -> Result<Vec<Config>, String> {
    read_configs_with_mode(mode).await
}

pub async fn clean_all_custom_hosts_entries_with_mode(mode: DatabaseMode) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    clean_all_custom_hosts_entries_with_pool(&context.pool).await
}

fn prepare_config(mut config: Config) -> Config {
    if let Some(ref mut alias) = config.alias {
        *alias = alias.trim().to_string();
    }
    if let Some(ref mut kubeconfig) = config.kubeconfig {
        *kubeconfig = kubeconfig.trim().to_string();
    }

    if config.local_port == Some(0) || config.local_port.is_none() {
        match pick_unused_port() {
            Some(port) => config.local_port = Some(port),
            None => {
                config.local_port = config.remote_port;
                error!("Failed to find an unused port, using remote_port as local_port");
            }
        }
    }

    if config.alias.as_deref() == Some("") || config.alias.is_none() {
        let workload_type = config.workload_type.clone().unwrap_or_default();
        let alias = format!(
            "{}-{}-{}",
            workload_type,
            config.protocol,
            config.local_port.unwrap_or_default()
        );
        config.alias = Some(alias);
    }

    if config.kubeconfig.as_deref() == Some("") || config.kubeconfig.is_none() {
        config.kubeconfig = Some("default".to_string());
    }

    if config.http_logs_enabled.is_none() {
        config.http_logs_enabled = Some(false);
    }
    if config.http_logs_max_file_size.is_none() {
        config.http_logs_max_file_size = Some(10 * 1024 * 1024); // 10MB
    }
    if config.http_logs_retention_days.is_none() {
        config.http_logs_retention_days = Some(7);
    }
    if config.http_logs_auto_cleanup.is_none() {
        config.http_logs_auto_cleanup = Some(true);
    }

    config
}

async fn sync_http_logs_config_from_config(
    config: &Config, config_id: i64, pool: &SqlitePool,
) -> Result<(), String> {
    use crate::models::http_logs_config_model::HttpLogsConfig;
    use crate::utils::http_logs_config::update_http_logs_config_with_pool;

    let http_config = HttpLogsConfig {
        config_id,
        enabled: config.http_logs_enabled.unwrap_or(false),
        max_file_size: config.http_logs_max_file_size.unwrap_or(10 * 1024 * 1024),
        retention_days: config.http_logs_retention_days.unwrap_or(7),
        auto_cleanup: config.http_logs_auto_cleanup.unwrap_or(true),
    };

    update_http_logs_config_with_pool(&http_config, pool).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use lazy_static::lazy_static;
    use serde_json::{
        Value,
        json,
    };
    use sqlx::SqlitePool;
    use tokio::sync::Mutex;

    use super::*;

    lazy_static! {
        static ref IO_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn test_prepare_config_trims_fields() {
        let config = Config {
            alias: Some("  alias  ".to_string()),
            kubeconfig: Some("  kube  ".to_string()),
            ..Config::default()
        };
        let prepared = prepare_config(config);
        assert_eq!(prepared.alias, Some("alias".to_string()));
        assert_eq!(prepared.kubeconfig, Some("kube".to_string()));
    }

    #[test]
    fn test_prepare_config_sets_default_kubeconfig() {
        let config_empty = Config {
            kubeconfig: Some("".to_string()),
            ..Config::default()
        };
        let prepared_empty = prepare_config(config_empty);
        assert_eq!(prepared_empty.kubeconfig, Some("default".to_string()));

        let config_none = Config {
            kubeconfig: None,
            ..Config::default()
        };
        let prepared_none = prepare_config(config_none);
        assert_eq!(prepared_none.kubeconfig, Some("default".to_string()));
    }

    #[test]
    fn test_prepare_config_sets_default_alias() {
        let config_empty = Config {
            alias: Some("".to_string()),
            workload_type: Some("deployment".to_string()),
            protocol: "TCP".to_string(),
            local_port: Some(8080),
            ..Config::default()
        };
        let prepared_empty = prepare_config(config_empty);
        assert_eq!(
            prepared_empty.alias,
            Some("deployment-TCP-8080".to_string())
        );

        let config_none = Config {
            alias: None,
            workload_type: Some("pod".to_string()),
            protocol: "UDP".to_string(),
            local_port: Some(9090),
            ..Config::default()
        };
        let prepared_none = prepare_config(config_none);
        assert_eq!(prepared_none.alias, Some("pod-UDP-9090".to_string()));
    }

    #[test]
    fn test_prepare_config_picks_local_port() {
        let config0 = Config {
            local_port: Some(0),
            remote_port: Some(8000),
            ..Config::default()
        };
        let prepared0 = prepare_config(config0);
        assert!(prepared0.local_port.is_some());
        assert_ne!(prepared0.local_port, Some(0));

        let config_none = Config {
            local_port: None,
            remote_port: Some(9000),
            ..Config::default()
        };
        let prepared_none = prepare_config(config_none);
        assert!(prepared_none.local_port.is_some());
    }

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        crate::migration::migrate_configs(Some(&pool))
            .await
            .expect("Failed to run migrations");
        pool
    }

    #[tokio::test]
    async fn test_insert_and_get_config() {
        let pool = setup_test_db().await;
        let config = Config {
            service: Some("test-service".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config.clone(), &pool)
            .await
            .unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        let retrieved_config = configs.first().unwrap();
        assert!(retrieved_config.id.is_some());
        let id = retrieved_config.id.unwrap();

        let fetched_config = get_config_with_pool(id, &pool).await.unwrap();
        assert_eq!(fetched_config.id, Some(id));
        assert_eq!(fetched_config.service, Some("test-service".to_string()));
        assert_eq!(fetched_config.namespace, config.namespace);
        assert!(fetched_config.local_port.is_some());
        assert_ne!(
            fetched_config.local_port,
            Some(0),
            "Local port should have been assigned by prepare_config"
        );
    }

    #[tokio::test]
    async fn test_read_multiple_configs() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("service1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("service2".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config1, &pool).await.unwrap();
        insert_config_with_pool(config2, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 2);
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("service1".to_string()))
        );
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("service2".to_string()))
        );
    }

    #[tokio::test]
    async fn test_get_config_not_found() {
        let pool = setup_test_db().await;
        let result = get_config_with_pool(999, &pool).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No config found with id: 999"));
    }

    #[tokio::test]
    async fn test_update_config() {
        let pool = setup_test_db().await;
        let config = Config {
            service: Some("initial-service".to_string()),
            ..Config::default()
        };
        insert_config_with_pool(config.clone(), &pool)
            .await
            .unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        let mut retrieved_config = configs.first().cloned().unwrap();
        let id = retrieved_config.id.unwrap();

        retrieved_config.service = Some("updated-service".to_string());
        update_config_with_pool(retrieved_config.clone(), &pool)
            .await
            .unwrap();

        let updated_config = get_config_with_pool(id, &pool).await.unwrap();
        assert_eq!(updated_config.id, Some(id));
        assert_eq!(updated_config.service, Some("updated-service".to_string()));
    }

    #[tokio::test]
    async fn test_delete_config() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("service1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("service2".to_string()),
            ..Config::default()
        };
        insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();

        let configs_before = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_before.len(), 2);
        let id_to_delete = configs_before
            .iter()
            .find(|c| c.service == Some("service1".to_string()))
            .unwrap()
            .id
            .unwrap();

        delete_config_with_pool(id_to_delete, &pool).await.unwrap();

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 1);
        assert_eq!(configs_after[0].service, Some("service2".to_string()));

        let result = get_config_with_pool(id_to_delete, &pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_configs() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("service1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("service2".to_string()),
            ..Config::default()
        };
        let config3 = Config {
            service: Some("service3".to_string()),
            ..Config::default()
        };
        insert_config_with_pool(config1, &pool).await.unwrap();
        insert_config_with_pool(config2, &pool).await.unwrap();
        insert_config_with_pool(config3, &pool).await.unwrap();

        let configs_before = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_before.len(), 3);
        let ids_to_delete: Vec<i64> = configs_before
            .iter()
            .filter(|c| {
                c.service == Some("service1".to_string())
                    || c.service == Some("service3".to_string())
            })
            .map(|c| c.id.unwrap())
            .collect();

        delete_configs_with_pool(ids_to_delete, &pool)
            .await
            .unwrap();

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 1);
        assert_eq!(configs_after[0].service, Some("service2".to_string()));
    }

    #[tokio::test]
    async fn test_delete_all_configs() {
        let pool = setup_test_db().await;
        insert_config_with_pool(Config::default(), &pool)
            .await
            .unwrap();
        insert_config_with_pool(Config::default(), &pool)
            .await
            .unwrap();

        assert_eq!(read_configs_with_pool(&pool).await.unwrap().len(), 2);

        delete_all_configs_with_pool(&pool).await.unwrap();

        assert_eq!(read_configs_with_pool(&pool).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_export_configs_refactored() {
        let pool = setup_test_db().await;
        let config1 = Config {
            service: Some("export-service1".to_string()),
            namespace: "default-namespace".to_string(),
            local_port: Some(8080),
            id: None,
            ..Config::default()
        };

        let config2 = Config {
            service: Some("export-service2".to_string()),
            namespace: "custom-ns".to_string(),
            alias: Some("".to_string()),
            id: None,
            ..Config::default()
        };

        insert_config_with_pool(config1.clone(), &pool)
            .await
            .unwrap();
        insert_config_with_pool(config2.clone(), &pool)
            .await
            .unwrap();

        let exported_json = export_configs_with_pool(&pool).await.unwrap();
        println!("Exported JSON: {exported_json}");

        let exported_configs: Vec<BTreeMap<String, Value>> =
            serde_json::from_str(&exported_json).expect("Failed to parse exported JSON");

        assert_eq!(exported_configs.len(), 2);

        let exported_c1 = exported_configs
            .iter()
            .find(|c| c.get("service").and_then(|v| v.as_str()) == Some("export-service1"))
            .expect("Config 1 not found in export");
        let exported_c2 = exported_configs
            .iter()
            .find(|c| c.get("service").and_then(|v| v.as_str()) == Some("export-service2"))
            .expect("Config 2 not found in export");

        assert_eq!(exported_c1.get("service"), Some(&json!("export-service1")));
        assert_eq!(exported_c1.get("local_port"), Some(&json!(8080)));
        assert!(
            exported_c1.get("namespace").is_none(),
            "Default namespace should be removed"
        );
        assert!(exported_c1.get("id").is_none(), "ID should not be present");

        assert_eq!(exported_c2.get("service"), Some(&json!("export-service2")));
        assert_eq!(exported_c2.get("namespace"), Some(&json!("custom-ns")));

        assert!(
            exported_c2.get("alias").is_some(),
            "Generated alias should be present"
        );
        let alias_from_json = exported_c2
            .get("alias")
            .unwrap()
            .as_str()
            .expect("Alias should be a string");
        assert_ne!(alias_from_json, "", "Generated alias should not be blank");
        assert_ne!(
            alias_from_json,
            Config::default().alias.unwrap(),
            "Generated alias should differ from default"
        );

        assert!(exported_c2.get("id").is_none(), "ID should not be present");
    }

    #[tokio::test]
    async fn test_import_configs_single() {
        let pool = setup_test_db().await;
        let config_json = json!({
            "service": "imported-service",
            "namespace": "import-ns",
            "local_port": 5000,
            "workload_type": "service",
            "protocol": "tcp",
            "context": "test-context"
        })
        .to_string();

        import_configs_with_pool(config_json, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        let imported = &configs[0];
        assert_eq!(imported.service, Some("imported-service".to_string()));
        assert_eq!(imported.namespace, "import-ns".to_string());
        assert_eq!(imported.local_port, Some(5000));
        assert!(imported.alias.is_some());
        assert_ne!(imported.alias.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn test_import_configs_multiple() {
        let pool = setup_test_db().await;
        let configs_json = json!([
            {
                "service": "imported-service1",
                "workload_type": "service",
                "protocol": "tcp",
                "context": "test-context",
                "namespace": "import-ns1",
                "local_port": 5001
            },
            {
                "service": "imported-service2",
                "workload_type": "service",
                "protocol": "tcp",
                "context": "test-context",
                "namespace": "import-ns2",
                "local_port": 5002
            }
        ])
        .to_string();

        import_configs_with_pool(configs_json, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 2);
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("imported-service1".to_string()))
        );
        assert!(
            configs
                .iter()
                .any(|c| c.service == Some("imported-service2".to_string()))
        );
    }

    #[tokio::test]
    async fn test_import_configs_invalid_json() {
        let pool = setup_test_db().await;
        let invalid_json = "{\"service\": \"bad json\",";
        let result = import_configs_with_pool(invalid_json.to_string(), &pool).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse config"));
    }

    #[tokio::test]
    async fn test_clean_all_custom_hosts_entries() {
        let _lock = IO_TEST_MUTEX.lock().await;
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("host-service1".to_string()),
            id: Some(1),
            ..Config::default()
        };

        let config2 = Config {
            service: Some("host-service2".to_string()),
            id: Some(2),
            ..Config::default()
        };

        insert_config_with_pool(config1, &pool).await.unwrap();
        insert_config_with_pool(config2, &pool).await.unwrap();

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let temp_path = temp_file.path();

        std::fs::write(temp_path, "# Test hosts file\n127.0.0.1 localhost\n").unwrap();

        let result =
            clean_all_custom_hosts_entries_with_pool_and_path(&pool, Some(temp_path)).await;
        assert!(
            result.is_ok(),
            "clean_all_custom_hosts_entries failed: {:?}",
            result.err()
        );

        let content = std::fs::read_to_string(temp_path).unwrap();
        assert!(content.contains("localhost"));
    }

    #[tokio::test]
    async fn test_get_configs() {
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("get-configs-test".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config1, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].service, Some("get-configs-test".to_string()));
    }

    #[tokio::test]
    async fn test_delete_config_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("delete-test-public".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        let id = configs[0].id.unwrap();

        let result = delete_config_with_pool(id, &pool).await;
        assert!(result.is_ok());

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_configs_public() {
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("delete-multi-1".to_string()),
            ..Config::default()
        };

        let config2 = Config {
            service: Some("delete-multi-2".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config1, &pool).await.unwrap();
        insert_config_with_pool(config2, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 2);
        let ids: Vec<i64> = configs.iter().map(|c| c.id.unwrap()).collect();

        let result = delete_configs_with_pool(ids, &pool).await;
        assert!(result.is_ok());

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_all_configs_public() {
        let pool = setup_test_db().await;

        let config1 = Config {
            service: Some("delete-all-1".to_string()),
            ..Config::default()
        };

        let config2 = Config {
            service: Some("delete-all-2".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config1, &pool).await.unwrap();
        insert_config_with_pool(config2, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 2);
        let result = delete_all_configs_with_pool(&pool).await;
        assert!(result.is_ok());

        let configs_after = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs_after.len(), 0);
    }

    #[tokio::test]
    async fn test_insert_config_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("insert-public-test".to_string()),
            ..Config::default()
        };

        let result = insert_config_with_pool(config.clone(), &pool).await;
        assert!(result.is_ok());

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].service, Some("insert-public-test".to_string()));
    }

    #[tokio::test]
    async fn test_read_configs_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("read-public-test".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config, &pool).await.unwrap();

        let result = read_configs_with_pool(&pool).await;
        assert!(result.is_ok());

        let configs = result.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].service, Some("read-public-test".to_string()));
    }

    #[tokio::test]
    async fn test_get_config_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("get-single-test".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        let id = configs[0].id.unwrap();

        let result = get_config_with_pool(id, &pool).await;
        assert!(result.is_ok());

        let fetched_config = result.unwrap();
        assert_eq!(fetched_config.service, Some("get-single-test".to_string()));
    }

    #[tokio::test]
    async fn test_get_config_not_found_public() {
        let pool = setup_test_db().await;

        let result = get_config_with_pool(999, &pool).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No config found with id: 999"));
    }

    #[tokio::test]
    async fn test_update_config_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("update-public-test".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(config, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        let mut config_to_update = configs[0].clone();

        config_to_update.service = Some("updated-service".to_string());

        let result = update_config_with_pool(config_to_update, &pool).await;
        assert!(result.is_ok());

        let updated_configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(updated_configs.len(), 1);
        assert_eq!(
            updated_configs[0].service,
            Some("updated-service".to_string())
        );
    }

    #[tokio::test]
    async fn test_export_configs_public() {
        let pool = setup_test_db().await;

        let config = Config {
            service: Some("export-public-test".to_string()),
            namespace: "test-namespace".to_string(),
            ..Config::default()
        };

        insert_config_with_pool(config, &pool).await.unwrap();

        let result = export_configs_with_pool(&pool).await;
        assert!(result.is_ok());

        let exported_json = result.unwrap();
        assert!(exported_json.contains("export-public-test"));
        assert!(exported_json.contains("test-namespace"));
    }

    #[tokio::test]
    async fn test_import_configs_public() {
        let pool = setup_test_db().await;

        let config_json = json!({
            "service": "import-public-test",
            "namespace": "import-namespace",
            "local_port": 5000,
            "workload_type": "service",
            "protocol": "tcp",
            "context": "test-context"
        })
        .to_string();

        let result = import_configs_with_pool(config_json, &pool).await;
        assert!(result.is_ok());

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].service, Some("import-public-test".to_string()));
        assert_eq!(configs[0].namespace, "import-namespace");
        assert_eq!(configs[0].local_port, Some(5000));
    }

    #[tokio::test]
    async fn test_prepare_config_port_fallback() {
        let config = Config {
            local_port: Some(0),
            remote_port: Some(8080),
            ..Config::default()
        };

        let prepared = prepare_config(config);

        assert!(prepared.local_port.is_some());

        if prepared.local_port == Some(8080) {
            assert_eq!(prepared.local_port, prepared.remote_port);
        } else {
            assert_ne!(prepared.local_port, Some(0));
        }
    }

    #[tokio::test]
    async fn test_error_reading_configs() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        let result = read_configs_with_pool(&pool).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_with_mode_memory() {
        use crate::utils::db_mode::DatabaseManager;
        DatabaseManager::cleanup_memory_pools();

        let config = Config {
            service: Some("memory-test".to_string()),
            ..Config::default()
        };

        insert_config_with_mode(config.clone(), DatabaseMode::Memory)
            .await
            .unwrap();

        let configs = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].service, Some("memory-test".to_string()));

        let config_id = configs[0].id.unwrap();
        let retrieved_config = get_config_with_mode(config_id, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(retrieved_config.service, Some("memory-test".to_string()));

        delete_config_with_mode(config_id, DatabaseMode::Memory)
            .await
            .unwrap();

        let configs_after_delete = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        assert_eq!(configs_after_delete.len(), 0);
    }

    #[tokio::test]
    async fn test_config_operations_with_mode_memory() {
        use crate::utils::db_mode::DatabaseManager;
        DatabaseManager::cleanup_memory_pools();

        let config1 = Config {
            service: Some("memory-test-1".to_string()),
            ..Config::default()
        };
        let config2 = Config {
            service: Some("memory-test-2".to_string()),
            ..Config::default()
        };

        insert_config_with_mode(config1, DatabaseMode::Memory)
            .await
            .unwrap();
        insert_config_with_mode(config2, DatabaseMode::Memory)
            .await
            .unwrap();

        let configs = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        assert_eq!(configs.len(), 2);

        let exported_json = export_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(exported_json.contains("memory-test-1"));
        assert!(exported_json.contains("memory-test-2"));

        delete_all_configs_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();

        let configs_after_delete = read_configs_with_mode(DatabaseMode::Memory).await.unwrap();
        assert_eq!(configs_after_delete.len(), 0);
    }
}
