use std::io;

use crossterm::{
    execute,
    terminal::{
        EnterAlternateScreen,
        LeaveAlternateScreen,
        disable_raw_mode,
        enable_raw_mode,
    },
};
use kftray_commons::utils::config::read_configs_with_mode;
use kftray_commons::utils::config_state::read_config_states_with_mode;
use kftray_commons::utils::config_view::ConfigView;
use kftray_commons::utils::db_mode::DatabaseMode;
use log::error;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
};
use tokio::task::JoinHandle;
use tokio::time::{
    self,
    Duration,
};

use crate::logging::LoggerState;
use crate::tui::input::{
    App,
    AppState,
    UpdateInfo,
    handle_input,
};
use crate::tui::ui::draw_ui;

type UpdateCheckTask = JoinHandle<Result<UpdateInfo, String>>;

pub(crate) use crate::core::port_forward::CLEANUP_RECONCILE_TIMEOUT;

pub async fn run_tui(
    mode: DatabaseMode, logger_state: LoggerState, _no_update_check: bool, config_view: ConfigView,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(logger_state);
    app.config_view = config_view;

    if let Ok(size) = terminal.size() {
        app.update_visible_rows(size.height);
    }

    #[cfg(not(debug_assertions))]
    let mut update_check: Option<UpdateCheckTask> = if !_no_update_check {
        Some(tokio::spawn(crate::updater::check_for_updates()))
    } else {
        None
    };
    #[cfg(debug_assertions)]
    let mut update_check: Option<UpdateCheckTask> = None;

    // Start network monitor if enabled
    let mut network_monitor_started = false;
    if let Ok(enabled) = kftray_commons::utils::settings::get_network_monitor_with_mode(mode).await
        && enabled
    {
        match kftray_network_monitor::start().await {
            Ok(()) => network_monitor_started = true,
            Err(e) => error!("Failed to start network monitor: {e}"),
        }
    }

    let res = run_app(&mut terminal, &mut app, mode, &mut update_check).await;

    // Stopped before the drain below, which can run for minutes: left alive,
    // the monitor's health check reacts to exactly the symptom shutdown
    // produces (forwards that stopped responding) by restarting them right
    // before the process exits, re-creating what cleanup just reaped.
    if network_monitor_started && let Err(e) = kftray_network_monitor::stop().await {
        error!("Failed to stop network monitor: {e}");
    }

    // Restore the terminal first: stopping every forward waits on recovery
    // locks and cluster deletions, and none of that should keep the shell in
    // raw mode. Every step is attempted even if an earlier one fails, and the
    // first error is held back rather than propagated, because returning here
    // would skip the cleanup that drains the pending-resource registry.
    let restored = [
        disable_raw_mode(),
        execute!(terminal.backend_mut(), LeaveAlternateScreen),
        terminal.show_cursor(),
    ]
    .into_iter()
    .find_map(Result::err);

    println!(
        "Stopping port forwards, this may take up to {}s…",
        CLEANUP_RECONCILE_TIMEOUT.as_secs() * 3
    );
    let failures = crate::core::port_forward::shutdown_port_forwarding(&mut app, mode).await;

    let mut had_failure = !failures.is_empty();

    if let Err(err) = res {
        error!("{err:?}");
        eprintln!("Error: the event loop failed unexpectedly: {err}");
        had_failure = true;
    }
    if let Some(error) = restored {
        error!("Failed to restore terminal: {error}");
        eprintln!("Error: failed to restore terminal: {error}");
        had_failure = true;
    }

    if had_failure {
        return Err("kftui did not shut down cleanly; see the errors above".into());
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>, app: &mut App, mode: DatabaseMode,
    update_check: &mut Option<UpdateCheckTask>,
) -> io::Result<()>
where
    std::io::Error: From<B::Error>,
{
    let mut interval = time::interval(Duration::from_millis(100));

    loop {
        let configs = read_configs_with_mode(mode).await.unwrap_or_default();
        let config_states = read_config_states_with_mode(mode).await.unwrap_or_default();

        app.update_configs(&configs, &config_states);
        app.load_http_logs_states(&configs, mode).await;
        app.load_active_pods(&config_states).await;

        if update_check.as_ref().is_some_and(|task| task.is_finished())
            && let Some(task) = update_check.take()
        {
            match task.await {
                Ok(Ok(update_info)) => {
                    app.update_prompt_pending = update_info.has_update;
                    app.update_info = Some(update_info);
                }
                Ok(Err(e)) => {
                    error!("Failed to check for updates: {e}");
                }
                Err(e) => {
                    error!("Update check task failed: {e}");
                }
            }
        }

        if app.update_prompt_pending && app.state == AppState::Normal {
            app.update_prompt_pending = false;
            app.state = AppState::ShowUpdateConfirmation;
        }

        if !app.logger_state.is_file_output_enabled() {
            tui_logger::move_events();
        }

        terminal.draw(|f| {
            draw_ui(f, app, &config_states);
        })?;

        if !app.configs_being_processed.is_empty() {
            app.throbber_state.calc_next();
        }

        if handle_input(app, mode).await? {
            break;
        }

        interval.tick().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::{
        config_model::Config,
        config_state_model::ConfigState,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::logging::{
        LogConfig,
        LoggerState,
    };
    use crate::tui::input::{
        ActiveComponent,
        ActiveTable,
        App,
        AppState,
    };
    use crate::tui::ui::draw_ui;

    fn test_logger_state() -> LoggerState {
        LoggerState::new(LogConfig::new(log::LevelFilter::Off))
    }

    /// Regression for a clamp that used to run unconditionally every frame:
    /// `app.error_scroll_max`/`app.error_scroll` were reset to 0 whenever
    /// any state other than `ShowErrorPopup` rendered, even though the error
    /// message and the reader's position in it were still logically live
    /// (e.g. a confirmation briefly covering an open error popup).
    #[test]
    fn error_scroll_state_is_preserved_when_another_popup_covers_the_error_popup() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(test_logger_state());
        app.error_message = Some("line 1\nline 2\nline 3\nline 4\nline 5".to_string());
        app.state = AppState::ShowErrorPopup;
        let config_states: Vec<ConfigState> = vec![];

        terminal
            .draw(|f| draw_ui(f, &mut app, &config_states))
            .unwrap();
        let max_after_first_render = app.error_scroll_max;
        assert!(
            max_after_first_render > 0,
            "a multi-line error must produce a nonzero scroll bound"
        );
        app.error_scroll = max_after_first_render;

        // A different popup temporarily covers the error popup.
        app.state = AppState::ShowDeleteConfirmation;
        terminal
            .draw(|f| draw_ui(f, &mut app, &config_states))
            .unwrap();
        assert_eq!(
            app.error_scroll_max, max_after_first_render,
            "covering the error popup with another must not reset its scroll bound"
        );
        assert_eq!(
            app.error_scroll, max_after_first_render,
            "covering the error popup with another must not reset the reader's position"
        );

        // Returning to the error popup must still reflect the preserved position.
        app.state = AppState::ShowErrorPopup;
        terminal
            .draw(|f| draw_ui(f, &mut app, &config_states))
            .unwrap();
        assert_eq!(app.error_scroll, max_after_first_render);
    }

    #[test]
    fn test_draw_ui_initial_state() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(test_logger_state());
        let config_states: Vec<ConfigState> = vec![];

        terminal
            .draw(|f| {
                draw_ui(f, &mut app, &config_states);
            })
            .unwrap();

        insta::assert_debug_snapshot!("initial_ui", terminal.backend().buffer());
    }

    #[test]
    fn test_draw_ui_with_data() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(test_logger_state());

        let configs = vec![
            Config {
                id: Some(1),
                service: Some("stopped-svc".to_string()),
                namespace: "ns1".to_string(),
                local_port: Some(8080),
                remote_port: Some(80),
                context: Some("ctx1".to_string()),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
            Config {
                id: Some(2),
                service: Some("running-svc".to_string()),
                namespace: "ns2".to_string(),
                local_port: Some(9090),
                remote_port: Some(90),
                context: Some("ctx2".to_string()),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
        ];

        let config_states = vec![
            ConfigState {
                id: Some(1),
                config_id: 1,
                is_running: false,
                process_id: None,
                ..Default::default()
            },
            ConfigState {
                id: Some(2),
                config_id: 2,
                is_running: true,
                process_id: Some(1234),
                ..Default::default()
            },
        ];

        app.update_configs(&configs, &config_states);
        app.table_state_stopped.select(Some(0));
        app.table_state_running.select(Some(0));
        app.active_component = ActiveComponent::StoppedTable;
        app.active_table = ActiveTable::Stopped;

        terminal
            .draw(|f| {
                draw_ui(f, &mut app, &config_states);
            })
            .unwrap();
        insta::assert_debug_snapshot!("ui_with_data", terminal.backend().buffer());

        app.active_component = ActiveComponent::RunningTable;
        app.active_table = ActiveTable::Running;
        terminal
            .draw(|f| {
                draw_ui(f, &mut app, &config_states);
            })
            .unwrap();
        insta::assert_debug_snapshot!("ui_with_data_running_active", terminal.backend().buffer());
    }
}
