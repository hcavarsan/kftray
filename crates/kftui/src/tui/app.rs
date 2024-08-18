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

    if let Err(e) = migrate_configs().await {
        error!("Failed to migrate configs: {}", e);
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

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>, app: &mut App,
) -> io::Result<()> {
    let mut interval = time::interval(Duration::from_millis(100));

    loop {
        let configs = read_configs().await.unwrap_or_default();
        let mut config_states = read_config_states().await.unwrap_or_default();

        app.update_configs(&configs, &config_states);
        fetch_new_logs(app).await;

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

async fn fetch_new_logs(app: &mut App) {
    let new_logs = {
        let mut buffer = app.stdout_output.lock().unwrap();
        let new_logs = buffer.clone();
        buffer.clear();
        new_logs
    };

    let mut log_content = app.log_content.lock().unwrap();
    let was_at_bottom = app.log_scroll_offset == app.log_scroll_max_offset;

    log_content.push_str(&new_logs);

    let log_lines: Vec<&str> = log_content.lines().collect();
    let log_height = app.visible_rows;
    app.log_scroll_max_offset = log_lines.len().saturating_sub(log_height);

    if was_at_bottom {
        app.log_scroll_offset = app.log_scroll_max_offset;
    }
}
