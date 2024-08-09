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
use kftray_commons::utils::config::read_configs;
use kftray_tauri::config_state::read_config_states;
use log::error;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

use crate::tui::input::{
    handle_input,
    App,
};
use crate::tui::ui::draw_ui;

pub async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the database
    kftray_commons::utils::db::init().await?;

    // Set up Crossterm
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create the app with the file explorer
    let mut app = App::new();

    // Run the TUI application
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
    loop {
        let configs = read_configs().await.unwrap_or_default();
        let mut config_states = read_config_states().await.unwrap_or_default();

        if !app.show_search {
            app.update_configs(&configs, &config_states);
        }

        terminal.draw(|f| {
            draw_ui(f, app, &config_states);
        })?;

        if handle_input(app, &mut config_states).await? {
            break;
        }
    }
    Ok(())
}
