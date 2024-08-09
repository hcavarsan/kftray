mod tui;
use log::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::set_logger(&*tui::logging::LOGGER).map(|()| log::set_max_level(LevelFilter::Trace))?;

    tui::run_tui().await
}
