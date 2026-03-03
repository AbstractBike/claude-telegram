mod config;
mod matrix;
mod session;
mod sandbox;
mod agent;
mod secrets;
mod observability;

use anyhow::Result;
use std::net::SocketAddr;
use crate::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Init structured JSON logging
    observability::logging::init();

    // 2. Load config from env var or default path
    let config_path = std::env::var("CLAUDE_CHAT_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| Config::default_path());

    let config = Config::load(&config_path)?;
    tracing::info!(
        config = %config_path.display(),
        rooms = config.rooms.agents.len(),
        "configuration loaded"
    );

    // 3. Register metric descriptions and start Prometheus exporter
    observability::metrics::register_metrics();
    let metrics_addr: SocketAddr = format!("0.0.0.0:{}", config.observability.metrics_port)
        .parse()?;
    observability::metrics::start_metrics_server(metrics_addr).await?;

    // 4. Set startup gauges
    metrics::gauge!("claude_chat_rooms_configured")
        .set(config.rooms.agents.len() as f64);

    let start_time = std::time::Instant::now();
    tokio::spawn(async move {
        loop {
            metrics::gauge!("claude_chat_uptime_seconds")
                .set(start_time.elapsed().as_secs_f64());
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
        }
    });

    // 5. Build and authenticate Matrix client
    let client = matrix::client::build_client(&config).await?;

    // 6. Run sync loop (blocks until error or shutdown)
    tracing::info!("bot ready, starting sync");
    matrix::client::run_sync(client).await?;

    Ok(())
}
