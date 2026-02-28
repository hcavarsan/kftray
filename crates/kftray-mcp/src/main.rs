//! KFtray MCP Server - Model Context Protocol server for LLM integration.
//!
//! This server exposes kftray functionality via the MCP protocol, allowing
//! LLMs to manage Kubernetes port-forwarding sessions.
//!
//! # Usage
//!
//! ```bash
//! # Start server on default port 3000
//! kftray-mcp
//!
//! # Start server on custom port
//! kftray-mcp --port 8080
//!
//! # Start server bound to specific address
//! kftray-mcp --host 0.0.0.0 --port 8080
//! ```
//!
//! # MCP Endpoints
//!
//! - `POST /mcp` - JSON-RPC requests
//! - `GET /mcp` - SSE event stream
//! - `DELETE /mcp` - Terminate session
//! - `GET /health` - Health check

use clap::Parser;
use log::{info, LevelFilter};
use std::net::{IpAddr, SocketAddr};

/// KFtray MCP Server - Kubernetes port-forwarding via Model Context Protocol
#[derive(Parser, Debug)]
#[command(
    name = "kftray-mcp",
    version,
    about = "MCP server for managing Kubernetes port-forwards",
    long_about = "A Model Context Protocol (MCP) server that exposes kftray functionality \
                  to LLMs, enabling AI-assisted Kubernetes port-forward management."
)]
struct Args {
    /// Host address to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: IpAddr,

    /// Port to listen on
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Log level (error, warn, info, debug, trace)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = match args.log_level.to_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };

    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp_secs()
        .init();

    let addr = SocketAddr::new(args.host, args.port);

    info!("Starting KFtray MCP Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Protocol version: {}", kftray_mcp::protocol::MCP_PROTOCOL_VERSION);
    info!("Listening on http://{}", addr);
    info!("");
    info!("Available tools:");
    for tool in kftray_mcp::tools::get_all_tools() {
        info!("  - {}: {}", tool.name, tool.description.unwrap_or_default());
    }
    info!("");
    info!("Endpoints:");
    info!("  POST   /mcp    - JSON-RPC requests");
    info!("  GET    /mcp    - SSE event stream");
    info!("  DELETE /mcp    - Terminate session");
    info!("  GET    /health - Health check");
    info!("");

    kftray_mcp::server::start_server(addr).await
}
