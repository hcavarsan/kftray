use std::io;

use crossterm::{
    execute,
    terminal::{
        disable_raw_mode,
        enable_raw_mode,
        EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use kftray_commons::config::read_configs;
use kftray_commons::utils::config_state::read_config_states;
use kftray_commons::utils::db::init;
use kftray_commons::utils::migration::migrate_configs;
use log::error;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use tokio::time::{
    self,
    Duration,
};

use crate::tui::input::{
    handle_input,
    App,
};
use crate::tui::ui::draw_ui;

pub async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    init().await?;

    if let Err(e) = migrate_configs(None).await {
        error!("Database migration failed: {}", e);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("{:?}", err);
    }

    Ok(())
}

pub async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>, app: &mut App,
) -> io::Result<()> {
    let mut interval = time::interval(Duration::from_millis(100));

    loop {
        let configs = read_configs().await.unwrap_or_default();
        let mut config_states = read_config_states().await.unwrap_or_default();

        app.update_configs(&configs, &config_states);

        terminal.draw(|f| {
            draw_ui(f, app, &config_states);
        })?;

        if handle_input(app, &mut config_states).await? {
            break;
        }

        interval.tick().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };

    use kftray_commons::models::{
        config_model::Config,
        config_state_model::ConfigState,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::tui::input::{
        ActiveComponent,
        ActiveTable,
        App,
    };
    use crate::tui::ui::draw_ui;

    static DB_INITIALIZED: AtomicBool = AtomicBool::new(false);

    fn initialize_test_db() {
        if !DB_INITIALIZED.load(Ordering::SeqCst) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                match init().await {
                    Ok(_) => DB_INITIALIZED.store(true, Ordering::SeqCst),
                    Err(e) => panic!("Failed to initialize DB for test: {:?}", e),
                }
            });
        }
    }

    #[test]
    fn test_draw_ui_initial_state() {
        initialize_test_db();
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
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
        initialize_test_db();
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();

        let configs = vec![
            Config {
                id: Some(1),
                service: Some("stopped-svc".to_string()),
                namespace: "ns1".to_string(),
                local_port: Some(8080),
                remote_port: Some(80),
                context: "ctx1".to_string(),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
            Config {
                id: Some(2),
                service: Some("running-svc".to_string()),
                namespace: "ns2".to_string(),
                local_port: Some(9090),
                remote_port: Some(90),
                context: "ctx2".to_string(),
                protocol: "tcp".to_string(),
                ..Default::default()
            },
        ];

        let config_states = vec![
            ConfigState {
                id: Some(1),
                config_id: 1,
                is_running: false,
            },
            ConfigState {
                id: Some(2),
                config_id: 2,
                is_running: true,
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
