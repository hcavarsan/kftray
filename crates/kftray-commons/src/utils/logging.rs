use std::sync::Once;

use tracing::Level;
use tracing_subscriber::{
    fmt::{
        self,
        time::UtcTime,
    },
    EnvFilter,
};

use crate::error::Result;
use crate::utils::paths;

// Used to ensure logger is initialized only once
static INIT: Once = Once::new();

pub async fn setup_logging(debug: bool) -> Result<()> {
    let log_path = paths::get_app_log_path().await?;

    // Create log directory if it doesn't exist
    if let Some(dir) = log_path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }

    let file = std::fs::File::create(log_path)?;

    INIT.call_once(|| {
        let env_filter = EnvFilter::from_default_env().add_directive(if debug {
            Level::DEBUG.into()
        } else {
            Level::INFO.into()
        });

        let subscriber = fmt::Subscriber::builder()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(false)
            .with_timer(UtcTime::rfc_3339())
            .with_writer(file)
            .compact()
            .try_init();

        if let Err(e) = subscriber {
            eprintln!("Failed to initialize logger: {}", e);
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use tracing::{
        debug,
        info,
    };

    use super::*;

    #[tokio::test]
    async fn test_logging_setup() {
        // Create a new temporary directory for each test
        let temp_dir = tempdir().unwrap();
        std::env::set_var("PORT_FORWARD_CONFIG", temp_dir.path());

        // Set up logging
        setup_logging(true).await.expect("Failed to setup logging");

        // Test logging
        info!("Test log message");
        debug!("Test debug message");

        let log_path = paths::get_app_log_path().await.unwrap();
        assert!(log_path.exists());

        // Clean up
        std::fs::remove_file(log_path).ok();
    }
}
