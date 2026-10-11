use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::time::Duration;

use clap::Parser;
use kftray_commons::models::config_model::Config;
use kftray_commons::test_utils::test_db;
use kftray_commons::utils::config::{
    delete_all_configs_with_mode,
    insert_config_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;

use crate::cli::args::Cli;
use crate::cli::watch::DiscoveryWatch;

const MODE: DatabaseMode = DatabaseMode::Memory;
const ROUND: Duration = Duration::from_millis(5);

fn annotated(service: &str, local_port: u16) -> Config {
    Config {
        context: Some("ctx".to_string()),
        namespace: "default".to_string(),
        service: Some(service.to_string()),
        alias: Some(service.to_string()),
        local_port: Some(local_port),
        remote_port: Some(80),
        protocol: "tcp".to_string(),
        workload_type: Some("service".to_string()),
        ..Config::default()
    }
}

/// Answers each discovery round with the next scripted result, then repeats
/// the last one.
fn scripted_watch(
    args: &[&str], rounds: Vec<Result<Vec<Config>, String>>,
) -> (DiscoveryWatch, Arc<AtomicUsize>) {
    let cli = Cli::parse_from(
        ["kftui", "--auto-discover", "--context", "ctx", "--watch"]
            .iter()
            .chain(args),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let mut rounds = VecDeque::from(rounds);
    let watch = DiscoveryWatch::spawn_with(cli, MODE, ROUND, move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let result = if rounds.len() > 1 {
            rounds.pop_front().unwrap()
        } else {
            rounds.front().cloned().unwrap()
        };
        async move { result }
    });
    (watch, calls)
}

async fn wait_for_rounds(calls: &AtomicUsize, rounds: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while calls.load(Ordering::SeqCst) < rounds {
            tokio::time::sleep(ROUND).await;
        }
    })
    .await
    .expect("the watch must keep running discovery rounds");
}

async fn saved_services() -> Vec<String> {
    let mut services: Vec<String> = read_configs_with_mode(MODE)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|config| config.service)
        .collect();
    services.sort();
    services
}

#[tokio::test]
async fn watch_adds_and_hands_over_only_newly_annotated_services() {
    let _db = test_db().await;
    delete_all_configs_with_mode(MODE).await.unwrap();
    insert_config_with_mode(annotated("api", 18080), MODE)
        .await
        .unwrap();

    let (mut watch, calls) = scripted_watch(
        &["--auto-start"],
        vec![
            Err("the cluster did not answer".to_string()),
            Ok(vec![annotated("api", 18080), annotated("web", 18081)]),
        ],
    );

    wait_for_rounds(&calls, 5).await;
    let to_start = watch.take_to_start();
    watch.stop(Duration::from_secs(1)).await;

    assert_eq!(saved_services().await, vec!["api", "web"]);
    let web_id = read_configs_with_mode(MODE)
        .await
        .unwrap()
        .into_iter()
        .find(|config| config.service.as_deref() == Some("web"))
        .and_then(|config| config.id)
        .unwrap();
    assert_eq!(to_start, vec![web_id]);
}

#[tokio::test]
async fn watch_without_auto_start_saves_new_services_but_starts_none() {
    let _db = test_db().await;
    delete_all_configs_with_mode(MODE).await.unwrap();

    let (mut watch, calls) = scripted_watch(&[], vec![Ok(vec![annotated("web", 18081)])]);

    wait_for_rounds(&calls, 3).await;
    let to_start = watch.take_to_start();
    watch.stop(Duration::from_secs(1)).await;

    assert_eq!(saved_services().await, vec!["web"]);
    assert!(to_start.is_empty());
}

#[tokio::test]
async fn stopped_watch_runs_no_more_rounds() {
    let _db = test_db().await;
    delete_all_configs_with_mode(MODE).await.unwrap();

    let (watch, calls) = scripted_watch(&[], vec![Ok(Vec::new())]);
    wait_for_rounds(&calls, 1).await;
    watch.stop(Duration::from_secs(1)).await;
    let after_stop = calls.load(Ordering::SeqCst);

    tokio::time::sleep(ROUND * 10).await;
    assert_eq!(calls.load(Ordering::SeqCst), after_stop);
}
