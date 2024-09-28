mod core;
mod tui;
mod utils;
use tui::run_tui;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tui_logger::init_logger(log::LevelFilter::Debug).unwrap();
    tui_logger::set_default_level(log::LevelFilter::Debug);

    run_tui().await
}
