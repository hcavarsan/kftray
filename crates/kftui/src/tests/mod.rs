// The test fixtures build synthetic configs and TUI layout snapshots that
// mix integer types freely (i64 row ids reused as ports, usize indices
// cast to terminal coordinates). The casts are deterministic and bounded
// by the test inputs, so the pedantic cast/precision/wrap warnings are
// noise in this module.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

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
