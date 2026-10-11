use std::sync::{
    Arc,
    Mutex,
};
use std::time::Duration;

use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::{
    insert_new_configs_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use log::{
    info,
    warn,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::cli::args::Cli;
use crate::cli::config::ConfigImporter;
use crate::cli::runner::PortForwardRunner;

pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(30);

/// Repeats auto-discovery in the background for `--watch`.
///
/// A round saves configs only for services that no saved config matches. With
/// `--auto-start`, the non-interactive mode starts them inside the round. The
/// TUI gets their ids through [`DiscoveryWatch::take_to_start`] and starts
/// them on its own task set, so its shutdown waits for those starts too.
pub struct DiscoveryWatch {
    cancel: CancellationToken,
    task: JoinHandle<()>,
    to_start: mpsc::UnboundedReceiver<Vec<i64>>,
    started: Arc<Mutex<Vec<i64>>>,
    non_interactive: bool,
}

impl DiscoveryWatch {
    pub fn spawn(cli: Cli, mode: DatabaseMode) -> Self {
        let discover_cli = cli.clone();
        Self::spawn_with(cli, mode, DISCOVERY_INTERVAL, move || {
            let cli = discover_cli.clone();
            async move { ConfigImporter::discover_annotated_configs(&cli).await }
        })
    }

    pub(crate) fn spawn_with<D, F>(
        cli: Cli, mode: DatabaseMode, interval: Duration, discover: D,
    ) -> Self
    where
        D: FnMut() -> F + Send + 'static,
        F: Future<Output = Result<Vec<Config>, String>> + Send + 'static,
    {
        let cancel = CancellationToken::new();
        let (sender, to_start) = mpsc::unbounded_channel();
        let started = Arc::new(Mutex::new(Vec::new()));
        let non_interactive = cli.non_interactive;
        let task = tokio::spawn(run_rounds(
            cli,
            mode,
            interval,
            discover,
            cancel.clone(),
            sender,
            Arc::clone(&started),
        ));
        Self {
            cancel,
            task,
            to_start,
            started,
            non_interactive,
        }
    }

    /// Ids of configs a round added for the TUI to start since the last call.
    pub fn take_to_start(&mut self) -> Vec<i64> {
        let mut ids = Vec::new();
        while let Ok(batch) = self.to_start.try_recv() {
            ids.extend(batch);
        }
        ids
    }

    /// Ends the rounds and returns the ids of the forwards they started.
    ///
    /// A round that is starting forwards gets the startup cancellation and
    /// up to `deadline` to finish. Past that it keeps running detached, and
    /// only the ids it already reported come back.
    pub async fn stop(self, deadline: Duration) -> Vec<i64> {
        let Self {
            cancel,
            task,
            started,
            non_interactive,
            ..
        } = self;
        cancel.cancel();
        if !task.is_finished() {
            kftray_portforward::kube::cancel_all_startups();
        }
        if tokio::time::timeout(deadline, task).await.is_err() {
            warn_user(
                non_interactive,
                "an auto-discovery round did not finish within the shutdown budget; \
                 forwards it was still starting are left to finish on their own",
            );
        }
        started.lock().map(|ids| ids.clone()).unwrap_or_default()
    }
}

async fn run_rounds<D, F>(
    cli: Cli, mode: DatabaseMode, interval: Duration, mut discover: D, cancel: CancellationToken,
    to_start: mpsc::UnboundedSender<Vec<i64>>, started: Arc<Mutex<Vec<i64>>>,
) where
    D: FnMut() -> F,
    F: Future<Output = Result<Vec<Config>, String>>,
{
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(interval) => {}
        }

        let discovered = tokio::select! {
            _ = cancel.cancelled() => return,
            discovered = discover() => discovered,
        };
        let added = match discovered {
            Ok(configs) => insert_new_configs_with_mode(configs, mode).await,
            Err(error) => Err(error),
        };
        let added = match added {
            Ok(added) => added,
            Err(error) => {
                warn_user(
                    cli.non_interactive,
                    &format!("auto-discovery round failed: {error}"),
                );
                continue;
            }
        };
        if added.is_empty() {
            continue;
        }
        say(
            cli.non_interactive,
            &format!(
                "Added {} configuration(s) from newly annotated services",
                added.len()
            ),
        );

        if !cli.auto_start || cancel.is_cancelled() {
            continue;
        }
        let ids = match read_configs_with_mode(mode).await {
            Ok(configs) => PortForwardRunner::select_config_ids(&cli, &configs, added),
            Err(error) => {
                warn_user(
                    cli.non_interactive,
                    &format!("failed to read the added configurations: {error}"),
                );
                continue;
            }
        };
        if ids.is_empty() {
            continue;
        }

        if cli.ssl
            && let Err(error) =
                PortForwardRunner::ensure_ssl_setup_with_configs(&ids, mode, cli.non_interactive)
                    .await
                    .map_err(|error| error.to_string())
        {
            warn_user(
                cli.non_interactive,
                &format!("SSL setup for the added configurations failed: {error}"),
            );
        }

        if cli.non_interactive {
            let ok = PortForwardRunner::start_discovered(&cli, mode, ids).await;
            if let Ok(mut started) = started.lock() {
                started.extend(ok);
            }
        } else if to_start.send(ids).is_err() {
            return;
        }
    }
}

fn say(non_interactive: bool, message: &str) {
    if non_interactive {
        println!("{message}");
    } else {
        info!("{message}");
    }
}

fn warn_user(non_interactive: bool, message: &str) {
    if non_interactive {
        eprintln!("Warning: {message}");
    } else {
        warn!("{message}");
    }
}
