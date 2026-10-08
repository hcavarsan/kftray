use clap::Parser;
use kftray_commons::test_utils::{
    EnvVarGuard,
    TestDb,
    test_db,
};
use kftray_commons::utils::config::{
    import_configs_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use tempfile::TempDir;

use crate::cli::args::Cli;
use crate::cli::config::ConfigImporter;

const SAVED: &str = r#"[{"alias":"keep-me","namespace":"default","service":"api","local_port":18080,"remote_port":80,"protocol":"tcp","workload_type":"service","context":"x"}]"#;

struct SavedConfigDb {
    dir: TempDir,
    _env: EnvVarGuard,
    _db: TestDb,
}

async fn db_with_saved_config() -> SavedConfigDb {
    let db = test_db().await;
    let dir = tempfile::tempdir().unwrap();
    let env = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
    kftray_commons::utils::db::init().await.unwrap();
    kftray_commons::utils::migration::migrate_configs(None)
        .await
        .unwrap();
    import_configs_with_mode(SAVED.to_string(), DatabaseMode::File)
        .await
        .unwrap();
    SavedConfigDb {
        dir,
        _env: env,
        _db: db,
    }
}

async fn saved_aliases() -> Vec<String> {
    read_configs_with_mode(DatabaseMode::File)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|c| c.alias)
        .collect()
}

async fn flush_import(args: &[&str]) -> Result<Vec<i64>, String> {
    let cli = Cli::parse_from(
        ["kftui", "--save", "--flush", "--non-interactive"]
            .iter()
            .chain(args),
    );
    ConfigImporter::import_configs(&cli, DatabaseMode::File).await
}

#[tokio::test]
async fn flush_keeps_saved_configs_when_json_is_invalid() {
    let _db = db_with_saved_config().await;

    assert!(flush_import(&["--json", "not json"]).await.is_err());

    assert_eq!(saved_aliases().await, vec!["keep-me"]);
}

#[tokio::test]
async fn flush_keeps_saved_configs_when_config_file_is_missing() {
    let db = db_with_saved_config().await;
    let missing = db.dir.path().join("missing.json");

    assert!(
        flush_import(&["--configs-path", missing.to_str().unwrap()])
            .await
            .is_err()
    );

    assert_eq!(saved_aliases().await, vec!["keep-me"]);
}

#[tokio::test]
async fn flush_keeps_saved_configs_when_a_config_fails_validation() {
    let _db = db_with_saved_config().await;
    let json = r#"[
        {"alias":"ok","namespace":"default","service":"web","remote_port":80,"protocol":"tcp","workload_type":"service","context":"x"},
        {"alias":"bad","namespace":"","service":"db","remote_port":5432,"protocol":"tcp","workload_type":"service","context":"x"}
    ]"#;

    assert!(flush_import(&["--json", json]).await.is_err());

    assert_eq!(saved_aliases().await, vec!["keep-me"]);
}

#[tokio::test]
async fn flush_replaces_saved_configs_after_a_valid_import() {
    let _db = db_with_saved_config().await;
    let json = r#"{"alias":"new","namespace":"default","service":"web","remote_port":80,"protocol":"tcp","workload_type":"service","context":"x"}"#;

    flush_import(&["--json", json]).await.unwrap();

    assert_eq!(saved_aliases().await, vec!["new"]);
}

#[tokio::test]
async fn import_returns_ids_of_saved_and_new_configs() {
    let _db = db_with_saved_config().await;
    let saved_id = read_configs_with_mode(DatabaseMode::File).await.unwrap()[0]
        .id
        .unwrap();
    let json = r#"[
        {"alias":"keep-me","namespace":"default","service":"api","local_port":18080,"remote_port":80,"protocol":"tcp","workload_type":"service","context":"x"},
        {"alias":"new","namespace":"default","service":"web","local_port":18081,"remote_port":80,"protocol":"tcp","workload_type":"service","context":"x"}
    ]"#;
    let cli = Cli::parse_from([
        "kftui",
        "--save",
        "--auto-start",
        "--non-interactive",
        "--json",
        json,
    ]);

    let ids = ConfigImporter::import_configs(&cli, DatabaseMode::File)
        .await
        .unwrap();

    let configs = read_configs_with_mode(DatabaseMode::File).await.unwrap();
    let new_id = configs
        .iter()
        .find(|c| c.alias.as_deref() == Some("new"))
        .and_then(|c| c.id)
        .unwrap();
    assert_eq!(configs.len(), 2);
    assert_eq!(ids, vec![saved_id, new_id]);
}
