use std::collections::BTreeMap;

use futures::stream::StreamExt;
use hostsfile::HostsBuilder;
use log::{
    error,
    info,
};
use portpicker::pick_unused_port;
use serde_json::{
    self,
    Value,
};
use serde_json::{
    json,
    Value as JsonValue,
};
use sqlx::{
    Row,
    SqlitePool,
};

use crate::db::{
    create_db_table,
    get_db_pool,
};
use crate::migration::migrate_configs;
use crate::models::config_model::Config;
use crate::utils::error::DbError;

pub(crate) async fn delete_config_with_pool(id: i64, pool: &SqlitePool) -> Result<(), DbError> {
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

pub(crate) async fn delete_configs_with_pool(
    ids: Vec<i64>, pool: &SqlitePool,
) -> Result<(), DbError> {
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

pub(crate) async fn delete_all_configs_with_pool(pool: &SqlitePool) -> Result<(), DbError> {
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

pub(crate) async fn insert_config_with_pool(
    config: Config, pool: &SqlitePool,
) -> Result<(), String> {
    let config = prepare_config(config);
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    create_db_table(pool).await.map_err(|e| e.to_string())?;

    let data = json!(config).to_string();
    sqlx::query("INSERT INTO configs (data) VALUES (?1)")
        .bind(data)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn insert_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    insert_config_with_pool(config, &pool).await
}

pub(crate) async fn read_configs_with_pool(pool: &SqlitePool) -> Result<Vec<Config>, String> {
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
        let hosts_builder = HostsBuilder::new(&hostfile_comment);
        let result = match custom_hosts_path {
            Some(path) => hosts_builder.write_to(path),
            None => hosts_builder.write(),
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
    Ok(())
}

pub async fn update_config(config: Config) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    update_config_with_pool(config, &pool).await
}

pub(crate) async fn export_configs_with_pool(pool: &SqlitePool) -> Result<String, String> {
    let mut configs = read_configs_with_pool(pool)
        .await
        .map_err(|e| e.to_string())?;
    for config in &mut configs {
        config.id = None;
        if config.namespace == "default-namespace" {
            config.namespace = "".to_string();
        }
    }

    let mut json_config = serde_json::to_value(configs).map_err(|e| e.to_string())?;
    let default_config = serde_json::to_value(Config::default()).map_err(|e| e.to_string())?;
    remove_blank_or_default_fields(&mut json_config, &default_config);

    let sorted_configs: Vec<BTreeMap<String, Value>> =
        serde_json::from_value(json_config).map_err(|e| e.to_string())?;

    let json = serde_json::to_string_pretty(&sorted_configs).map_err(|e| e.to_string())?;

    Ok(json)
}

pub async fn export_configs() -> Result<String, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    export_configs_with_pool(&pool).await
}

pub(crate) async fn import_configs_with_pool(
    json: String, pool: &SqlitePool,
) -> Result<(), String> {
    let configs: Vec<Config> = match serde_json::from_str(&json) {
        Ok(configs) => configs,
        Err(e) => {
            info!("Failed to parse JSON as Vec<Config>: {e}. Trying as single Config.");
            let config = serde_json::from_str::<Config>(&json)
                .map_err(|e| format!("Failed to parse config: {e}"))?;
            vec![config]
        }
    };

    for config in configs {
        insert_config_with_pool(config, pool)
            .await
            .map_err(|e| format!("Failed to insert config: {e}"))?;
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

fn is_value_default(value: &serde_json::Value, default_config: &serde_json::Value) -> bool {
    *value == *default_config
}

fn is_value_blank(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

fn remove_blank_or_default_fields(value: &mut JsonValue, default_config: &JsonValue) {
    match value {
        JsonValue::Object(map) => {
            let keys_to_remove: Vec<String> = map
                .iter()
                .filter(|(k, v)| {
                    let default_v = default_config.get(k);
                    match default_v {
                        Some(def_v) => is_value_blank(v) || is_value_default(v, def_v),
                        None => is_value_blank(v),
                    }
                })
                .map(|(k, _)| k.clone())
                .collect();

            for key in keys_to_remove {
                map.remove(&key);
            }

            for (key, value) in map.iter_mut() {
                if let Some(sub_default) = default_config.get(key) {
                    remove_blank_or_default_fields(value, sub_default);
                } else {
                    remove_blank_or_default_fields(value, &JsonValue::Null);
                }
            }
        }
        JsonValue::Array(arr) => {
            for value in arr {
                remove_blank_or_default_fields(value, &JsonValue::Null);
            }
        }
        _ => (),
    }
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

    config
}

#[cfg(test)]
mod tests {

    use lazy_static::lazy_static;
    use serde_json::json;
    use sqlx::SqlitePool;
    use tokio::sync::Mutex;

    use super::*;

    lazy_static! {
        static ref IO_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn test_is_value_blank_string() {
        assert!(is_value_blank(&json!("")));
        assert!(is_value_blank(&json!("  ")));
        assert!(!is_value_blank(&json!("not blank")));
    }

    #[test]
    fn test_is_value_blank_non_string() {
        assert!(!is_value_blank(&json!(123)));
        assert!(!is_value_blank(&json!(null)));
        assert!(!is_value_blank(&json!(true)));
        assert!(!is_value_blank(&json!([])));
        assert!(!is_value_blank(&json!({})));
    }

    #[test]
    fn test_is_value_default() {
        let default_val = json!({ "key": "default" });
        assert!(is_value_default(&json!({ "key": "default" }), &default_val));
        assert!(!is_value_default(
            &json!({ "key": "not default" }),
            &default_val
        ));
        assert!(!is_value_default(&json!(123), &default_val));
    }

    #[test]
    fn test_remove_blank_or_default_fields_simple() {
        let mut obj = json!({ "a": "", "b": "  ", "c": "value", "d": 123, "e": null });
        let default = json!({ "a": "default", "b": "default", "c": "default", "d": 0, "e": null });
        remove_blank_or_default_fields(&mut obj, &default);
        assert_eq!(obj, json!({ "c": "value", "d": 123 }));
    }

    #[test]
    fn test_remove_blank_or_default_fields_nested() {
        let mut obj = json!({
            "level1": {
                "a": "",
                "b": "default_b",
                "c": [1, 2, ""],
                "d": { "e": "", "f": "default_f" }
            },
            "g": "default_g"
        });
        let default = json!({
            "level1": {
                "a": "default_a",
                "b": "default_b",
                "c": [],
                "d": { "e": "default_e", "f": "default_f" }
            },
            "g": "default_g"
        });
        remove_blank_or_default_fields(&mut obj, &default);
        assert_eq!(
            obj,
            json!({
                "level1": {
                     "c": [1, 2, ""],
                     "d": {}
                }
            })
        );
    }

    #[test]
    fn test_remove_blank_or_default_fields_array() {
        let mut arr = json!([{"a": "", "b": "val"}, {"a": "default_a", "b": ""}]);
        let default = json!({ "a": "default_a", "b": "default_b"});
        remove_blank_or_default_fields(&mut arr, &default);
        assert_eq!(arr[0], json!({ "b": "val"}));
        assert_eq!(arr[1], json!({ "a": "default_a"}));
        assert_eq!(arr[1], json!({ "a": "default_a"}));
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
        assert!(configs
            .iter()
            .any(|c| c.service == Some("service1".to_string())));
        assert!(configs
            .iter()
            .any(|c| c.service == Some("service2".to_string())));
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
            "local_port": 5000
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
                "namespace": "import-ns1",
                "local_port": 5001
            },
            {
                "service": "imported-service2",
                "namespace": "import-ns2",
                "local_port": 5002
            }
        ])
        .to_string();

        import_configs_with_pool(configs_json, &pool).await.unwrap();

        let configs = read_configs_with_pool(&pool).await.unwrap();
        assert_eq!(configs.len(), 2);
        assert!(configs
            .iter()
            .any(|c| c.service == Some("imported-service1".to_string())));
        assert!(configs
            .iter()
            .any(|c| c.service == Some("imported-service2".to_string())));
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
            "local_port": 5000
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
}
