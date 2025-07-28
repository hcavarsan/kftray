#![allow(clippy::needless_return)]
pub mod cli;
pub mod core;
pub mod logging;
pub mod stdin;
pub mod tui;
pub mod utils;

pub use tui::app::run_tui;

#[cfg(test)]
mod tests;
