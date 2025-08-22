use clap::{
    Parser,
    Subcommand,
};
use kftray_helper::{
    platforms::{
        install_platform_service,
        run_platform_service,
        uninstall_platform_service,
    },
    HelperClient,
};
use log::{
    info,
    warn,
};

#[derive(Parser)]
#[command(author, version, about = "KFTray privileged helper")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Install {
        #[arg(short, long)]
        service_name: Option<String>,
    },

    Uninstall {
        #[arg(short, long)]
        service_name: Option<String>,
    },

    Service,

    Status,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Install { service_name } => {
            let service = service_name
                .clone()
                .unwrap_or_else(|| "kftray.helper".to_string());
            install_platform_service(&service)?;
            info!("Service installed successfully");
        }
        Commands::Uninstall { service_name } => {
            let service = service_name
                .clone()
                .unwrap_or_else(|| "kftray.helper".to_string());
            uninstall_platform_service(&service)?;
            info!("Service uninstalled successfully");
        }
        Commands::Service => {
            info!("Starting platform service...");
            run_platform_service()?;
        }
        Commands::Status => {
            let app_id = "com.kftray.app".to_string();
            let client = HelperClient::new(app_id)?;

            if client.is_helper_available() {
                match client.ping() {
                    Ok(true) => {
                        info!("Helper service is running and responding");
                        std::process::exit(0);
                    }
                    _ => {
                        warn!("Helper service socket exists but is not responding properly");
                        std::process::exit(1);
                    }
                }
            } else {
                info!("Helper service is not running");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
