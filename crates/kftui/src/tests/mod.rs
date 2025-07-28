pub mod test_app;
pub mod test_draw;
pub mod test_file_explorer;
pub mod test_input;
pub mod test_navigation;
pub mod test_popup;
pub mod test_popup_functions;
pub mod test_snapshots;
pub mod test_ui;
pub mod test_ui_popup;
pub mod test_utils_config;
pub mod test_utils_file;

use crate::logging::{
    LogConfig,
    LoggerState,
};

pub fn test_logger_state() -> LoggerState {
    LoggerState::new(LogConfig::new(log::LevelFilter::Off))
}
