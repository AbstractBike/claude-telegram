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
    // LocalSet enables spawn_local for the Temporal worker (Worker is !Send due to Rc internals)
    tokio::task::LocalSet::new().run_until(run()).await
}

async fn run() -> Result<()> {
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

    // 5. Build Matrix client once — shared between worker activities and sync loop
    let matrix_client = Arc::new(loop {
        match matrix::client::build_client(&config).await {
            Ok(c) => {
                tracing::info!(user = %config.matrix.user, "logged in to Matrix");
                break c;
            }
            Err(e) => {
                tracing::error!(error = %e, "Matrix login failed, retrying in 10s");
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        }
    });

    // 6. Connect to Temporal, start workflows, and spawn the worker
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

                // Spawn the worker that polls the task queue and executes activities/workflows
                // Worker is !Send (uses Rc for determinism) — must use spawn_local
                let worker_client = matrix_client.clone();
                let worker_config = config.clone();
                tokio::task::spawn_local(async move {
                    match temporal::worker::start_worker(worker_config, worker_client).await {
                        Ok(mut worker) => {
                            tracing::info!("temporal worker started, polling task queue");
                            if let Err(e) = worker.run().await {
                                tracing::error!(error = %e, "temporal worker stopped unexpectedly");
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "failed to start temporal worker"),
                    }
                });

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

    // 7. Run Matrix sync loop — re-login on token expiry, backoff on other errors
    tracing::info!("bot ready, starting sync");
    let mut backoff_secs = 1u64;

    loop {
        match matrix::client::run_sync((*matrix_client).clone(), config.clone(), temporal_dispatcher.clone()).await {
            Ok(_) => {
                tracing::warn!("sync loop returned unexpectedly, restarting");
                backoff_secs = 1;
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::error!(error = %msg, "sync loop exited, reconnecting");

                if msg.contains("M_UNKNOWN_TOKEN") || msg.contains("Invalid access token") {
                    tracing::info!("token expired — re-logging in");
                    match matrix::client::re_login(&matrix_client, &config).await {
                        Ok(_) => { backoff_secs = 1; continue; }
                        Err(re) => tracing::error!(error = %re, "re-login failed"),
                    }
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(120);
    }
}
