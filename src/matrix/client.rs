use anyhow::Result;
use matrix_sdk::{Client, config::SyncSettings};
use metrics::{counter, histogram};
use std::time::Instant;

use crate::config::Config;

/// Derive session ID from room alias (e.g. "#nixos-agent:matrix.pin" -> "nixos-agent")
pub fn derive_session_id(alias: &str) -> String {
    let stripped = alias.strip_prefix('#').unwrap_or(alias);
    stripped.split(':').next().unwrap_or(stripped).to_string()
}

/// Build and authenticate a Matrix client from configuration.
pub async fn build_client(config: &Config) -> Result<Client> {
    let client = Client::builder()
        .homeserver_url(&config.matrix.homeserver)
        .build()
        .await?;

    let password = config.matrix_password()?;
    client
        .matrix_auth()
        .login_username(&config.matrix.user, &password)
        .initial_device_display_name("claude-chat")
        .await?;

    tracing::info!(user = %config.matrix.user, "logged in to Matrix");
    Ok(client)
}

/// Run the Matrix sync loop. This blocks indefinitely until an error occurs.
pub async fn run_sync(client: Client) -> Result<()> {
    tracing::info!("starting Matrix sync loop");
    let start = Instant::now();
    match client.sync(SyncSettings::default()).await {
        Ok(_) => Ok(()),
        Err(e) => {
            let elapsed = start.elapsed().as_secs_f64();
            histogram!("claude_chat_matrix_sync_duration_seconds").record(elapsed);
            counter!("claude_chat_matrix_sync_errors_total", "error" => e.to_string())
                .increment(1);
            Err(e.into())
        }
    }
}
