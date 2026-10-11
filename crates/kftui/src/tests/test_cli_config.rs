use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

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
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};

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

async fn answer_api_requests(mut stream: TcpStream, listed_namespaces: Arc<AtomicBool>) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            }
        }
        let end = buffer.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&buffer[..end]).to_string();
        buffer.drain(..end);
        let target = head.split_whitespace().nth(1).unwrap_or_default();
        let path = target.split('?').next().unwrap_or_default().to_string();
        if path == "/api/v1/namespaces" {
            listed_namespaces.store(true, Ordering::SeqCst);
        }

        let (status, body) = match path.as_str() {
            "/version" => ("200 OK", serde_json::json!({"major": "1", "minor": "30"})),
            "/api/v1/namespaces/team-a/services" => (
                "200 OK",
                serde_json::json!({
                    "kind": "ServiceList",
                    "apiVersion": "v1",
                    "metadata": {},
                    "items": [{
                        "metadata": {
                            "name": "web",
                            "namespace": "team-a",
                            "annotations": {
                                "kftray.app/enabled": "true",
                                "kftray.app/configs": "web-18090-80"
                            }
                        },
                        "spec": {"ports": [{"port": 80, "targetPort": 80}]}
                    }]
                }),
            ),
            _ => (
                "403 Forbidden",
                serde_json::json!({
                    "kind": "Status",
                    "apiVersion": "v1",
                    "status": "Failure",
                    "message": "forbidden",
                    "reason": "Forbidden",
                    "code": 403
                }),
            ),
        };
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        if stream.write_all(response.as_bytes()).await.is_err() {
            return;
        }
    }
}

#[tokio::test]
async fn auto_discover_with_namespace_imports_without_listing_namespaces() {
    kftray_portforward::ssl::ensure_crypto_provider_installed();
    let db = db_with_saved_config().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let listed_namespaces = Arc::new(AtomicBool::new(false));
    let server_flag = Arc::clone(&listed_namespaces);
    let server = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(answer_api_requests(stream, Arc::clone(&server_flag)));
        }
    });
    let kubeconfig = db.dir.path().join("kubeconfig");
    std::fs::write(
        &kubeconfig,
        format!(
            "apiVersion: v1\nkind: Config\ncurrent-context: dev\nclusters:\n- name: fake\n  \
             cluster:\n    server: http://{address}\ncontexts:\n- name: dev\n  context:\n    \
             cluster: fake\n    user: dev\nusers:\n- name: dev\n  user:\n    token: fake\n"
        ),
    )
    .unwrap();
    let cli = Cli::parse_from([
        "kftui",
        "--save",
        "--auto-discover",
        "--context",
        "dev",
        "--kubeconfig",
        kubeconfig.to_str().unwrap(),
        "--namespace",
        "team-a",
    ]);

    ConfigImporter::import_configs(&cli, DatabaseMode::File)
        .await
        .unwrap();
    server.abort();

    let imported: Vec<_> = read_configs_with_mode(DatabaseMode::File)
        .await
        .unwrap()
        .into_iter()
        .filter(|c| c.alias.as_deref() == Some("web"))
        .map(|c| (c.namespace, c.local_port))
        .collect();
    assert_eq!(imported, vec![("team-a".to_string(), Some(18090))]);
    assert!(!listed_namespaces.load(Ordering::SeqCst));
}
