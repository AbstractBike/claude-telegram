mod config;
mod matrix;
mod session;
mod sandbox;
mod agent;
mod secrets;
mod observability;
mod temporal;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::config::Config;

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("mcp-vault") {
        let vault_root = flag_value(&args, "--vault-root")
            .ok_or_else(|| anyhow::anyhow!("--vault-root required"))?;
        let agent = flag_value(&args, "--agent")
            .ok_or_else(|| anyhow::anyhow!("--agent required"))?;
        return secrets::stdio_server::run_stdio_server(&vault_root, &agent).await;
    }

    // 1. Init structured JSON logging
    observability::logging::init();

    // 2. Load config from env var or default path
    let config_path = std::env::var("CLAUDE_CHAT_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| Config::default_path());

    let config = Arc::new(Config::load(&config_path)?);
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

    // 5. Connect to Temporal and start workflows if configured
    let temporal_dispatcher = if config.temporal.is_some() {
        match temporal::client::TemporalDispatcher::new(&config).await {
            Ok(dispatcher) => {
                let dispatcher = Arc::new(dispatcher);
                for (name, agent_cfg) in &config.rooms.agents {
                    let wf_input = temporal::client::TemporalDispatcher::build_workflow_input(
                        name, agent_cfg, &config,
                    );
                    if let Err(e) = dispatcher.ensure_running(name, wf_input).await {
                        tracing::warn!(agent = %name, error = %e, "failed to ensure workflow running");
                    }
                }
                Some(dispatcher)
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to connect to Temporal, running without it");
                None
            }
        }
    } else {
        None
    };

    // 6. Connect and run Matrix — retry forever on connection failures
    let mut backoff_secs = 1u64;
    let max_backoff_secs = 120u64;

    loop {
        match matrix::client::build_client(&config).await {
            Ok(client) => {
                tracing::info!("bot ready, starting sync");
                backoff_secs = 1;

                // run_sync loops forever internally; if it returns, something went wrong
                if let Err(e) = matrix::client::run_sync(client, config.clone(), temporal_dispatcher.clone()).await {
                    tracing::error!(error = %e, "sync loop exited with error, reconnecting");
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    backoff_secs,
                    "failed to connect to Matrix, retrying"
                );
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(max_backoff_secs);
    }
}
