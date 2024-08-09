mod file_explorer;
mod navigation;
mod popup;

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use crossterm::event::{
    self,
    Event,
    KeyCode,
    KeyModifiers,
};
pub use file_explorer::*;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
pub use popup::*;
use ratatui_explorer::{
    FileExplorer,
    Theme,
};

use crate::tui::input::navigation::{
    start_port_forwarding,
    stop_port_forwarding,
};
use crate::tui::input::popup::handle_search_input;
use crate::tui::logging::LOGGER;
pub struct App {
    pub selected_rows_stopped: std::collections::HashSet<usize>,
    pub selected_rows_running: std::collections::HashSet<usize>,
    pub file_explorer: FileExplorer,
    pub file_explorer_open: bool,
    pub selected_row_stopped: usize,
    pub selected_row_running: usize,
    pub active_table: ActiveTable,
    pub import_export_message: Option<String>,
    pub show_input_prompt: bool,
    pub input_buffer: String,
    pub selected_file_path: Option<std::path::PathBuf>,
    pub show_confirmation_popup: bool,
    pub file_content: Option<String>,
    pub show_help: bool,
    pub stopped_configs: Vec<Config>,
    pub running_configs: Vec<Config>,
    pub filtered_stopped_configs: Vec<Config>,
    pub filtered_running_configs: Vec<Config>,
    pub stdout_output: Arc<Mutex<String>>,
    pub show_search: bool,
    pub show_error_popup: bool,
    pub error_message: Option<String>,
    pub active_component: ActiveComponent,
    pub selected_menu_item: usize,
}

#[derive(PartialEq)]
pub enum ActiveComponent {
    Menu,
    Table,
}

impl App {
    pub fn new() -> Self {
        let theme = Theme::default().add_default_title();
        let file_explorer = FileExplorer::with_theme(theme).unwrap();
        let stdout_output = LOGGER.buffer.clone();
        Self {
            file_explorer,
            file_explorer_open: false,
            selected_row_stopped: 0,
            selected_row_running: 0,
            active_table: ActiveTable::Stopped,
            selected_rows_stopped: std::collections::HashSet::new(),
            selected_rows_running: std::collections::HashSet::new(),
            import_export_message: None,
            show_input_prompt: false,
            input_buffer: String::new(),
            selected_file_path: None,
            show_confirmation_popup: false,
            file_content: None,
            show_help: false,
            stopped_configs: Vec::new(),
            running_configs: Vec::new(),
            filtered_stopped_configs: Vec::new(),
            filtered_running_configs: Vec::new(),
            stdout_output,
            show_search: false,
            show_error_popup: false,
            error_message: None,
            active_component: ActiveComponent::Menu,
            selected_menu_item: 0,
        }
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

        self.filtered_stopped_configs = self.stopped_configs.clone();
        self.filtered_running_configs = self.running_configs.clone();
    }
}

#[derive(PartialEq)]
pub enum ActiveTable {
    Stopped,
    Running,
}

pub async fn handle_input(app: &mut App, config_states: &mut [ConfigState]) -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(true);
            }

            if app.show_error_popup {
                handle_error_popup_input(app, key.code);
            } else if app.show_confirmation_popup {
                handle_confirmation_popup_input(app, key.code).await;
            } else if app.file_explorer_open {
                handle_file_explorer_input(app, key.code).await?;
            } else if app.show_input_prompt {
                handle_input_prompt_input(app, key.code).await;
            } else if app.show_help {
                handle_help_input(app, key.code);
            } else if app.show_search {
                handle_search_input(app, key.code).await;
            } else {
                match app.active_component {
                    ActiveComponent::Menu => match key.code {
                        KeyCode::Tab => {
                            app.active_component = ActiveComponent::Table;
                        }
                        KeyCode::Left => {
                            if app.selected_menu_item > 0 {
                                app.selected_menu_item -= 1;
                            }
                        }
                        KeyCode::Right => {
                            if app.selected_menu_item < 3 {
                                app.selected_menu_item += 1;
                            }
                        }
                        KeyCode::Enter => match app.selected_menu_item {
                            0 => {
                                app.file_explorer_open = true;
                                app.selected_file_path = None;
                                app.show_input_prompt = false;
                            }
                            1 => {
                                app.file_explorer_open = true;
                                app.selected_file_path = None;
                                app.show_input_prompt = true;
                            }
                            2 => {
                                app.show_help = true;
                            }
                            3 => {
                                return Ok(true);
                            }
                            _ => {}
                        },
                        _ => {}
                    },
                    ActiveComponent::Table => match key.code {
                        KeyCode::Tab => {
                            app.active_component = ActiveComponent::Menu;
                        }
                        KeyCode::Up => match app.active_table {
                            ActiveTable::Stopped => {
                                if !app.stopped_configs.is_empty() {
                                    app.selected_row_stopped = if app.selected_row_stopped == 0 {
                                        app.stopped_configs.len() - 1
                                    } else {
                                        app.selected_row_stopped - 1
                                    };
                                }
                            }
                            ActiveTable::Running => {
                                if !app.running_configs.is_empty() {
                                    app.selected_row_running = if app.selected_row_running == 0 {
                                        app.running_configs.len() - 1
                                    } else {
                                        app.selected_row_running - 1
                                    };
                                }
                            }
                        },
                        KeyCode::Down => match app.active_table {
                            ActiveTable::Stopped => {
                                if !app.stopped_configs.is_empty() {
                                    app.selected_row_stopped =
                                        (app.selected_row_stopped + 1) % app.stopped_configs.len();
                                }
                            }
                            ActiveTable::Running => {
                                if !app.running_configs.is_empty() {
                                    app.selected_row_running =
                                        (app.selected_row_running + 1) % app.running_configs.len();
                                }
                            }
                        },
                        KeyCode::Left => {
                            app.active_table = ActiveTable::Stopped;
                            app.selected_rows_running.clear();
                        }
                        KeyCode::Right => {
                            app.active_table = ActiveTable::Running;
                            app.selected_rows_stopped.clear();
                        }
                        KeyCode::Char(' ') => {
                            let selected_row = match app.active_table {
                                ActiveTable::Stopped => app.selected_row_stopped,
                                ActiveTable::Running => app.selected_row_running,
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
                        KeyCode::Char('c') => {
                            let mut stdout_output = app.stdout_output.lock().unwrap();
                            stdout_output.clear();
                        }
                        KeyCode::Char('f') => {
                            let (selected_rows, configs) = match app.active_table {
                                ActiveTable::Stopped => {
                                    (&app.selected_rows_stopped, &app.stopped_configs)
                                }
                                ActiveTable::Running => {
                                    (&app.selected_rows_running, &app.running_configs)
                                }
                            };

                            let selected_configs: Vec<Config> = selected_rows
                                .iter()
                                .filter_map(|&row| configs.get(row).cloned())
                                .collect();

                            if app.active_table == ActiveTable::Stopped {
                                for config in selected_configs.clone() {
                                    if let Some(state) = config_states
                                        .iter_mut()
                                        .find(|s| s.config_id == config.id.unwrap_or_default())
                                    {
                                        if !state.is_running {
                                            start_port_forwarding(app, config.clone()).await;
                                            state.is_running = true;
                                        }
                                    }
                                }
                            } else if app.active_table == ActiveTable::Running {
                                for config in selected_configs.clone() {
                                    if let Some(state) = config_states
                                        .iter_mut()
                                        .find(|s| s.config_id == config.id.unwrap_or_default())
                                    {
                                        if state.is_running {
                                            stop_port_forwarding(app, config.clone()).await;
                                            state.is_running = false;
                                        }
                                    }
                                }
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
                        }
                        KeyCode::Char('s') => {
                            app.show_search = true;
                        }
                        KeyCode::Char('h') => {
                            app.show_help = true;
                        }
                        KeyCode::Char('q') => {
                            return Ok(true);
                        }
                        _ => {}
                    },
                }
            }
        }
    }
    Ok(false)
}
