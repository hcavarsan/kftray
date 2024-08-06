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
use kftray_tauri::config::read_configs;
use kftray_tauri::config_state::read_config_states;
use log::error;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

use crate::tui::input::handle_input;
use crate::tui::ui::draw_ui;

/// Entry point for running the TUI application.
pub async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the database
    kftray_tauri::db::init().await?;

    // Set up Crossterm
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the TUI application
    let res = run_app(&mut terminal).await;

    // Restore the terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("{:?}", err);
    }

    Ok(())
}

/// Main loop for running the TUI application.
async fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    let mut selected_row = 0;
    let mut show_details = false;

    loop {
        // Fetch configs and states
        let configs = read_configs().await.unwrap_or_default();
        let mut config_states = read_config_states().await.unwrap_or_default();

        // Draw the UI
        terminal.draw(|f| {
            draw_ui(f, &configs, &config_states, selected_row, show_details);
        })?;

        // Handle input
        if handle_input(
            &mut selected_row,
            &mut show_details,
            configs.len(),
            &configs,
            &mut config_states,
        )
        .await?
        {
            break;
        }
    }
    Ok(())
}
