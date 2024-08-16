mod file_explorer;
mod navigation;
mod popup;
use std::collections::HashSet;
use std::io;
use std::sync::{
    Arc,
    Mutex,
};

use crossterm::event::{
    self,
    Event,
    KeyCode,
    KeyModifiers,
};
use crossterm::terminal::size;
pub use file_explorer::*;
use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};
pub use popup::*;
use ratatui::widgets::TableState;
use ratatui_explorer::{
    FileExplorer,
    Theme,
};

use crate::core::logging::LOGGER;
use crate::core::port_forward::stop_all_port_forward_and_exit;
use crate::tui::input::navigation::handle_port_forward;

#[derive(PartialEq, Clone, Copy)]
pub enum DeleteButton {
    Confirm,
    Close,
}

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveComponent {
    Menu,
    Table,
}

#[derive(PartialEq)]
pub enum ActiveTable {
    Stopped,
    Running,
}

#[derive(PartialEq)]
pub enum AppState {
    Normal,
    ShowErrorPopup,
    ShowConfirmationPopup,
    ImportFileExplorerOpen,
    ExportFileExplorerOpen,
    ShowInputPrompt,
    ShowHelp,
    ShowAbout,
    ShowDeleteConfirmation,
}

pub struct App {
    pub selected_rows_stopped: HashSet<usize>,
    pub selected_rows_running: HashSet<usize>,
    pub import_file_explorer: FileExplorer,
    pub export_file_explorer: FileExplorer,
    pub state: AppState,
    pub selected_row_stopped: usize,
    pub selected_row_running: usize,
    pub active_table: ActiveTable,
    pub import_export_message: Option<String>,
    pub input_buffer: String,
    pub selected_file_path: Option<std::path::PathBuf>,
    pub file_content: Option<String>,
    pub stopped_configs: Vec<Config>,
    pub running_configs: Vec<Config>,
    pub stdout_output: Arc<Mutex<String>>,
    pub error_message: Option<String>,
    pub active_component: ActiveComponent,
    pub selected_menu_item: usize,
    pub delete_confirmation_message: Option<String>,
    pub selected_delete_button: DeleteButton,
    pub visible_rows: usize,
    pub table_state_stopped: TableState,
    pub table_state_running: TableState,
}

impl App {
    pub fn new() -> Self {
        let theme = Theme::default().add_default_title();
        let import_file_explorer = FileExplorer::with_theme(theme.clone()).unwrap();
        let export_file_explorer = FileExplorer::with_theme(theme).unwrap();
        let stdout_output = LOGGER.buffer.clone();

        let mut app = Self {
            import_file_explorer,
            export_file_explorer,
            state: AppState::Normal,
            selected_row_stopped: 0,
            selected_row_running: 0,
            active_table: ActiveTable::Stopped,
            selected_rows_stopped: HashSet::new(),
            selected_rows_running: HashSet::new(),
            import_export_message: None,
            input_buffer: String::new(),
            selected_file_path: None,
            file_content: None,
            stopped_configs: Vec::new(),
            running_configs: Vec::new(),
            stdout_output,
            error_message: None,
            active_component: ActiveComponent::Table,
            selected_menu_item: 0,
            delete_confirmation_message: None,
            selected_delete_button: DeleteButton::Confirm,
            visible_rows: 0,
            table_state_stopped: TableState::default(),
            table_state_running: TableState::default(),
        };

        if let Ok((_, height)) = size() {
            app.update_visible_rows(height);
        }

        app
    }

    pub fn update_visible_rows(&mut self, terminal_height: u16) {
        self.visible_rows = (terminal_height.saturating_sub(19)) as usize;
    }

    pub fn update_configs(&mut self, configs: &[Config], config_states: &[ConfigState]) {
        self.stopped_configs = configs
            .iter()
            .filter(|config| {
                config_states
                    .iter()
                    .find(|state| state.config_id == config.id.unwrap_or_default())
                    .map(|state| !state.is_running)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        self.running_configs = configs
            .iter()
            .filter(|config| {
                config_states
                    .iter()
                    .find(|state| state.config_id == config.id.unwrap_or_default())
                    .map(|state| state.is_running)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
    }

    pub fn scroll_up(&mut self) {
        match self.active_table {
            ActiveTable::Stopped => {
                if let Some(selected) = self.table_state_stopped.selected() {
                    if selected > 0 {
                        self.table_state_stopped.select(Some(selected - 1));
                    }
                }
            }
            ActiveTable::Running => {
                if let Some(selected) = self.table_state_running.selected() {
                    if selected > 0 {
                        self.table_state_running.select(Some(selected - 1));
                    }
                }
            }
        }
    }

    pub fn scroll_down(&mut self) {
        match self.active_table {
            ActiveTable::Stopped => {
                if let Some(selected) = self.table_state_stopped.selected() {
                    if selected < self.stopped_configs.len() - 1 {
                        self.table_state_stopped.select(Some(selected + 1));
                    }
                } else {
                    self.table_state_stopped.select(Some(0));
                }
            }
            ActiveTable::Running => {
                if let Some(selected) = self.table_state_running.selected() {
                    if selected < self.running_configs.len() - 1 {
                        self.table_state_running.select(Some(selected + 1));
                    }
                } else {
                    self.table_state_running.select(Some(0));
                }
            }
        }
    }

    pub fn reset_scroll(&mut self) {
        self.table_state_stopped.select(Some(0));
        self.table_state_running.select(Some(0));
    }
}

fn toggle_select_all(app: &mut App) {
    let (selected_rows, configs) = match app.active_table {
        ActiveTable::Stopped => (&mut app.selected_rows_stopped, &app.stopped_configs),
        ActiveTable::Running => (&mut app.selected_rows_running, &app.running_configs),
    };

    if selected_rows.len() == configs.len() {
        selected_rows.clear();
    } else {
        selected_rows.clear();
        for i in 0..configs.len() {
            selected_rows.insert(i);
        }
    }
}

pub async fn handle_input(app: &mut App, _config_states: &mut [ConfigState]) -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                stop_all_port_forward_and_exit(app).await;
            }

            match app.state {
                AppState::ShowErrorPopup => {
                    handle_error_popup_input(app, key.code)?;
                }
                AppState::ShowConfirmationPopup => {
                    handle_confirmation_popup_input(app, key.code).await?;
                }
                AppState::ImportFileExplorerOpen => {
                    handle_import_file_explorer_input(app, key.code).await?;
                }
                AppState::ExportFileExplorerOpen => {
                    handle_export_file_explorer_input(app, key.code).await?;
                }
                AppState::ShowInputPrompt => {
                    handle_export_input_prompt(app, key.code).await?;
                }
                AppState::ShowHelp => {
                    handle_help_input(app, key.code)?;
                }
                AppState::ShowAbout => {
                    handle_about_input(app, key.code)?;
                }
                AppState::ShowDeleteConfirmation => {
                    handle_delete_confirmation_input(app, key.code).await?;
                }
                AppState::Normal => {
                    handle_normal_input(app, key.code).await?;
                }
            }
        } else if let Event::Resize(_, height) = event::read()? {
            app.update_visible_rows(height);
        }
    }
    Ok(false)
}

async fn handle_normal_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match app.active_component {
        ActiveComponent::Menu => match key {
            KeyCode::Tab => app.active_component = ActiveComponent::Table,
            KeyCode::Left => {
                if app.selected_menu_item > 0 {
                    app.selected_menu_item -= 1
                }
            }
            KeyCode::Right => {
                if app.selected_menu_item < 4 {
                    app.selected_menu_item += 1
                }
            }
            KeyCode::Enter => match app.selected_menu_item {
                0 => app.state = AppState::ShowHelp,
                1 => open_import_file_explorer(app),
                2 => open_export_file_explorer(app),
                3 => app.state = AppState::ShowAbout,
                4 => stop_all_port_forward_and_exit(app).await,
                _ => {}
            },
            _ => {}
        },
        ActiveComponent::Table => match key {
            KeyCode::Tab => app.active_component = ActiveComponent::Menu,
            KeyCode::Left => switch_to_stopped_table(app),
            KeyCode::Right => switch_to_running_table(app),
            KeyCode::Char(' ') => toggle_row_selection(app),
            KeyCode::Char('c') => clear_stdout_output(app),
            KeyCode::Char('f') => handle_port_forwarding(app).await?,
            KeyCode::Char('q') => app.state = AppState::ShowAbout,
            KeyCode::Char('i') => open_import_file_explorer(app),
            KeyCode::Char('e') => open_export_file_explorer(app),
            KeyCode::Char('h') => app.state = AppState::ShowHelp,
            KeyCode::Char('d') => show_delete_confirmation(app),
            KeyCode::Up => app.scroll_up(),
            KeyCode::Down => app.scroll_down(),
            KeyCode::Char('a') => toggle_select_all(app),
            _ => {}
        },
    }
    Ok(())
}

fn switch_to_stopped_table(app: &mut App) {
    app.active_table = ActiveTable::Stopped;
    app.reset_scroll();
    app.selected_rows_running.clear();
}

fn switch_to_running_table(app: &mut App) {
    app.active_table = ActiveTable::Running;
    app.reset_scroll();
    app.selected_rows_stopped.clear();
}

fn toggle_row_selection(app: &mut App) {
    let selected_row = match app.active_table {
        ActiveTable::Stopped => app.table_state_stopped.selected().unwrap_or(0),
        ActiveTable::Running => app.table_state_running.selected().unwrap_or(0),
    };

    let selected_rows = match app.active_table {
        ActiveTable::Stopped => &mut app.selected_rows_stopped,
        ActiveTable::Running => &mut app.selected_rows_running,
    };

    if selected_rows.contains(&selected_row) {
        selected_rows.remove(&selected_row);
    } else {
        selected_rows.insert(selected_row);
    }
}

fn clear_stdout_output(app: &mut App) {
    let mut stdout_output = app.stdout_output.lock().unwrap();
    stdout_output.clear();
}

async fn handle_port_forwarding(app: &mut App) -> io::Result<()> {
    let (selected_rows, configs, selected_row) = match app.active_table {
        ActiveTable::Stopped => (
            &mut app.selected_rows_stopped,
            &app.stopped_configs,
            app.selected_row_stopped,
        ),
        ActiveTable::Running => (
            &mut app.selected_rows_running,
            &app.running_configs,
            app.selected_row_running,
        ),
    };

    if selected_rows.is_empty() {
        selected_rows.insert(selected_row);
    }

    let selected_configs: Vec<Config> = selected_rows
        .iter()
        .filter_map(|&row| configs.get(row).cloned())
        .collect();

    for config in selected_configs.clone() {
        handle_port_forward(app, config).await;
    }

    if app.active_table == ActiveTable::Stopped {
        app.running_configs.extend(selected_configs.clone());
        app.stopped_configs
            .retain(|config| !selected_configs.contains(config));
    } else {
        app.stopped_configs.extend(selected_configs.clone());
        app.running_configs
            .retain(|config| !selected_configs.contains(config));
    }

    match app.active_table {
        ActiveTable::Stopped => app.selected_rows_stopped.clear(),
        ActiveTable::Running => app.selected_rows_running.clear(),
    }

    Ok(())
}

fn show_delete_confirmation(app: &mut App) {
    if !app.selected_rows_stopped.is_empty() {
        app.state = AppState::ShowDeleteConfirmation;
        app.delete_confirmation_message =
            Some("Are you sure you want to delete the selected configs?".to_string());
    }
}

async fn handle_delete_confirmation_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Left | KeyCode::Right => {
            app.selected_delete_button = match app.selected_delete_button {
                DeleteButton::Confirm => DeleteButton::Close,
                DeleteButton::Close => DeleteButton::Confirm,
            };
        }
        KeyCode::Enter => {
            if app.selected_delete_button == DeleteButton::Confirm {
                let ids_to_delete: Vec<i64> = app
                    .selected_rows_stopped
                    .iter()
                    .filter_map(|&row| app.stopped_configs.get(row).and_then(|config| config.id))
                    .collect();

                match kftray_commons::utils::config::delete_configs(ids_to_delete.clone()).await {
                    Ok(_) => {
                        app.delete_confirmation_message =
                            Some("Configs deleted successfully.".to_string());
                        app.stopped_configs.retain(|config| {
                            !ids_to_delete.contains(&config.id.unwrap_or_default())
                        });
                    }
                    Err(e) => {
                        app.delete_confirmation_message =
                            Some(format!("Failed to delete configs: {}", e));
                    }
                }
            }
            app.selected_rows_stopped.clear();
            app.state = AppState::Normal;
        }
        KeyCode::Esc => app.state = AppState::Normal,
        _ => {}
    }
    Ok(())
}

fn open_import_file_explorer(app: &mut App) {
    app.state = AppState::ImportFileExplorerOpen;
    app.selected_file_path = std::env::current_dir().ok();
}

fn open_export_file_explorer(app: &mut App) {
    app.state = AppState::ExportFileExplorerOpen;
    app.selected_file_path = std::env::current_dir().ok();
}
