pub(crate) mod test_app;
pub(crate) mod test_draw;
pub(crate) mod test_file_explorer;
pub(crate) mod test_input;
pub(crate) mod test_navigation;
pub(crate) mod test_popup;
pub(crate) mod test_popup_functions;
pub(crate) mod test_snapshots;
pub(crate) mod test_ui;
pub(crate) mod test_ui_popup;
pub(crate) mod test_utils_config;
pub(crate) mod test_utils_file;

use crate::logging::{
    LogConfig,
    LoggerState,
};

pub(crate) fn test_logger_state() -> LoggerState {
    LoggerState::new(LogConfig::new(log::LevelFilter::Off))
}
