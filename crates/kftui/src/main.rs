#![allow(clippy::needless_return)]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    kftui::run().await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };

    static TUI_LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn test_initialize_logger() {
        if !TUI_LOGGER_INITIALIZED.load(Ordering::SeqCst) {
            let result = tui_logger::init_logger(log::LevelFilter::Debug);
            TUI_LOGGER_INITIALIZED.store(result.is_ok(), Ordering::SeqCst);
        }

        tui_logger::set_default_level(log::LevelFilter::Debug);
    }
}
