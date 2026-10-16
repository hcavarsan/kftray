use std::time::Duration;

use clap::Parser;
use kftray_commons::test_utils::{
    EnvVarGuard,
    test_db,
};
use kftray_commons::utils::config::import_configs_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::settings::get_ssl_enabled;

use crate::cli::CliHandler;
use crate::cli::args::Cli;
use crate::cli::runner::PortForwardRunner;
use crate::logging::{
    LogConfig,
    LoggerState,
};

#[cfg(unix)]
#[tokio::test]
async fn run_command_exits_with_the_code_of_the_command() {
    let _db = test_db().await;
    let cases: [(&[&str], i32); 4] = [
        (&["sh", "-c", "exit 0"], 0),
        (&["sh", "-c", "exit 3"], 3),
        (&["sh", "-c", "kill -9 $$"], 137),
        (&["kftui-no-such-command"], 127),
    ];
    for (command, expected) in cases {
        let command: Vec<String> = command.iter().map(|arg| arg.to_string()).collect();
        let code = PortForwardRunner::run_command(&command, Vec::new(), None, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(code, expected, "exit code of {command:?}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_command_does_not_run_when_no_forward_starts() {
    kftray_portforward::ssl::ensure_crypto_provider_installed();
    let _db = test_db().await;
    let dir = tempfile::tempdir().unwrap();
    let _env = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let kubeconfig = dir.path().join("kubeconfig");
    std::fs::write(
        &kubeconfig,
        format!(
            "apiVersion: v1\nkind: Config\ncurrent-context: dev\nclusters:\n- name: fake\n  \
             cluster:\n    server: http://{closed}\ncontexts:\n- name: dev\n  context:\n    \
             cluster: fake\n    user: dev\nusers:\n- name: dev\n  user:\n    token: fake\n"
        ),
    )
    .unwrap();
    let configs = serde_json::json!([{
        "alias": "web",
        "service": "web",
        "namespace": "default",
        "local_port": 0,
        "remote_port": 80,
        "protocol": "tcp",
        "workload_type": "service",
        "context": "dev",
        "kubeconfig": kubeconfig,
    }])
    .to_string();
    let marker = dir.path().join("ran");
    let cli = Cli::parse_from([
        "kftui",
        "--json",
        &configs,
        "--auto-start",
        "--",
        "touch",
        marker.to_str().unwrap(),
    ]);

    let result = CliHandler::new(cli, LoggerState::new(LogConfig::new(log::LevelFilter::Off)))
        .run()
        .await;

    assert!(result.is_err());
    assert!(!marker.exists());
}

#[tokio::test]
async fn ssl_without_auto_start_turns_on_https_before_the_tui_opens() {
    let _db = test_db().await;
    let dir = tempfile::tempdir().unwrap();
    let _config = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
    let _test_mode = EnvVarGuard::set("KFTRAY_TEST_MODE", "1");
    let _skip_ca_install = EnvVarGuard::set("KFTRAY_SKIP_CA_INSTALL", "1");
    *kftray_portforward::ssl::cert_store::TEST_SSL_VAULT
        .lock()
        .unwrap() = Default::default();
    kftray_commons::utils::db::init().await.unwrap();
    kftray_commons::utils::migration::migrate_configs(None)
        .await
        .unwrap();
    import_configs_with_mode(
        r#"[{"alias":"web","namespace":"default","service":"web","local_port":18443,"remote_port":80,"protocol":"tcp","workload_type":"service","context":"dev"}]"#.to_string(),
        DatabaseMode::File,
    )
    .await
    .unwrap();

    let cli = Cli::parse_from(["kftui", "--ssl"]);
    CliHandler::new(cli, LoggerState::new(LogConfig::new(log::LevelFilter::Off)))
        .prepare_tui_forwards(Vec::new())
        .await
        .unwrap();

    assert!(get_ssl_enabled().await.unwrap());
    assert!(
        std::fs::read_dir(dir.path().join("ssl-certs"))
            .unwrap()
            .next()
            .is_some()
    );
}

/// A stop still in flight when `drain_stop_tasks`'s deadline elapses must
/// keep running to completion, detached, rather than being aborted: only the
/// future joining its `JoinHandle` is dropped, not the spawned task doing
/// the actual stop work (releasing a relay, an address claim, etc).
/// `stop_all_port_forwards` relies on this by spawning every stop with
/// `tokio::spawn` before handing the handles to `drain_stop_tasks`.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_detaches_unfinished_spawned_stops_when_the_deadline_elapses() {
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let handle = tokio::spawn(async move {
        release_rx.await.expect("release must be sent");
        done_tx.send(()).expect("receiver must still be listening");
        Ok::<(), String>(())
    });

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(vec![(1, handle)], Duration::from_millis(50)).await;

    assert!(!drained_ok, "the deadline must be reported as exceeded");
    assert!(
        results.is_empty(),
        "an unfinished stop reports no result to this caller"
    );
    assert!(completed_ids.is_empty());

    release_tx
        .send(())
        .expect("the spawned stop must still be alive, not aborted, after the deadline");

    tokio::time::timeout(Duration::from_secs(1), done_rx)
        .await
        .expect("must not time out waiting on a still-running detached stop")
        .expect("the detached stop must run to completion and report back");
}

/// Companion to the detach test: a stop that finishes comfortably inside the
/// deadline must still report its result and id normally.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_reports_results_that_finish_in_time() {
    let handle = tokio::spawn(async { Ok::<(), String>(()) });

    let (results, completed_ids, drained_ok) =
        PortForwardRunner::drain_stop_tasks(vec![(7, handle)], Duration::from_secs(5)).await;

    assert!(drained_ok);
    assert_eq!(results, vec![(7, Ok(()))]);
    assert!(completed_ids.contains(&7));
}

/// Every handle passed to `drain_stop_tasks` was already spawned by the
/// caller before the call, so a slow sibling timing out must not discard a
/// result a faster stop already reported: the two run concurrently on the
/// runtime from the moment they were spawned, not one after another gated by
/// how `drain_stop_tasks` happens to await them.
#[tokio::test(start_paused = true)]
async fn drain_stop_tasks_preserves_a_finished_result_when_a_later_sibling_times_out() {
    let fast_handle = tokio::spawn(async { Ok::<(), String>(()) });

    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let slow_handle = tokio::spawn(async move {
        release_rx.await.expect("release must be sent");
        Ok::<(), String>(())
    });

    let (results, completed_ids, drained_ok) = PortForwardRunner::drain_stop_tasks(
        vec![(1, fast_handle), (2, slow_handle)],
        Duration::from_millis(50),
    )
    .await;

    assert!(!drained_ok, "the slow stop must be reported as unfinished");
    assert_eq!(
        results,
        vec![(1, Ok(()))],
        "the fast stop's result must survive its slower sibling timing out"
    );
    assert!(completed_ids.contains(&1));
    assert!(
        !completed_ids.contains(&2),
        "the slow stop must not be reported as completed"
    );

    let _ = release_tx.send(());
}
