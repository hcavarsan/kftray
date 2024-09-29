#![allow(clippy::needless_return)]
mod core;
mod tui;
mod utils;

use core::logging::init_logger;

use tui::run_tui;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger()?;
    run_tui().await
}
