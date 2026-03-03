use anyhow::Result;
use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, SyncRoomMessageEvent,
    },
};
use metrics::{counter, histogram};
use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::matrix::handler::{classify_message, check_auth, AuthResult, MessageSource};
use crate::matrix::sender::send_text;
use crate::session::claude::ClaudeSession;

/// Write `.mcp.json` into the agent's work_dir so Claude CLI picks up the vault MCP server.
async fn write_mcp_config(work_dir: &str, agent_name: &str, vault_root: &str) {
    let claude_chat_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "claude-chat".to_string());

    let config = serde_json::json!({
        "mcpServers": {
            "vault": {
                "command": claude_chat_bin,
                "args": ["mcp-vault", "--vault-root", vault_root, "--agent", agent_name]
            }
        }
    });

    let path = std::path::Path::new(work_dir).join(".mcp.json");
    if let Err(e) = tokio::fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).await {
        tracing::warn!(agent = %agent_name, error = %e, "failed to write .mcp.json");
    } else {
        tracing::debug!(agent = %agent_name, path = %path.display(), "wrote .mcp.json");
    }
}

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

/// Run the Matrix sync loop with message event handler.
pub async fn run_sync(client: Client, config: Arc<Config>) -> Result<()> {
    tracing::info!("starting Matrix sync loop");

    let own_user_id = config.matrix.user.clone();

    client.add_event_handler({
        let config = config.clone();
        let own_user = own_user_id.clone();
        move |event: SyncRoomMessageEvent, room: Room| {
            let config = config.clone();
            let own_user = own_user.clone();
            async move {
                let event = match event {
                    SyncRoomMessageEvent::Original(e) => e,
                    _ => return,
                };
                if let Err(e) = handle_room_message(event, room, &config, &own_user).await {
                    tracing::error!(error = %e, "failed to handle message");
                }
            }
        }
    });

    // Initial sync to avoid processing old messages
    let response = client.sync_once(SyncSettings::default()).await?;
    tracing::info!("initial sync complete, listening for new messages");

    // Now sync continuously from the token after initial sync
    let settings = SyncSettings::default().token(response.next_batch);
    let start = Instant::now();
    match client.sync(settings).await {
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

async fn handle_room_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: &Config,
    own_user: &str,
) -> Result<()> {
    // Ignore own messages
    if event.sender.as_str() == own_user {
        return Ok(());
    }

    let body = match &event.content.msgtype {
        MessageType::Text(text) => text.body.clone(),
        _ => return Ok(()),
    };

    let room_id = room.room_id().to_string();
    let sender = event.sender.to_string();

    counter!("claude_chat_matrix_messages_received_total",
        "room" => room_id.clone()).increment(1);

    tracing::info!(
        room = %room_id,
        sender = %sender,
        len = body.len(),
        "message received"
    );

    // Find which agent config matches this room
    let agent_entry = config.rooms.agents.iter().find(|(_, ac)| ac.room_id == room_id);

    let (agent_name, agent_config) = match agent_entry {
        Some((name, ac)) => (name.clone(), ac),
        None => {
            // Check if it's the control room
            if room_id == config.rooms.control.room_id {
                tracing::info!(room = %room_id, "control room message, not yet implemented");
                return Ok(());
            }
            tracing::warn!(room = %room_id, "message from unknown room, ignoring");
            return Ok(());
        }
    };

    // Auth check
    let allowed = agent_config.effective_allowed_users(&config.auth.default_allowed_users);
    match check_auth(&sender, allowed) {
        AuthResult::Allowed => {}
        AuthResult::Denied => {
            tracing::warn!(user = %sender, agent = %agent_name, "unauthorized message");
            send_text(&room, "Access denied.").await?;
            return Ok(());
        }
    }

    // Classify message
    let source = classify_message(&body, config.inter_agent.max_depth);

    match source {
        MessageSource::ControlCommand(cmd) => {
            tracing::info!(agent = %agent_name, cmd = %cmd, "control command in agent room");
            send_text(&room, &format!("Control commands should be sent to the control room. Got: {cmd}")).await?;
        }
        MessageSource::DepthExceeded { from, depth } => {
            tracing::warn!(agent = %agent_name, from = %from, depth, "inter-agent depth exceeded");
            send_text(&room, &format!("Inter-agent depth limit exceeded (depth={depth}, from={from})")).await?;
        }
        MessageSource::UserMessage(text) | MessageSource::AgentMessage { text, .. } => {
            tracing::info!(agent = %agent_name, "spawning Claude session");

            if agent_config.encrypt {
                if let Some(ref vault) = config.vault {
                    write_mcp_config(&agent_config.work_dir, &agent_name, &vault.root).await;
                }
            }

            let session = ClaudeSession::new_sandboxed(
                agent_name.clone(),
                agent_config.work_dir.clone(),
                agent_config.store_dir.clone(),
                agent_config.timeout(),
                std::env::var("CLAUDE_PATH").unwrap_or_else(|_| "claude".to_string()),
            );

            let response = session.send_raw(&text).await?;
            send_text(&room, &response).await?;

            tracing::info!(agent = %agent_name, response_len = response.len(), "response sent");
        }
    }

    Ok(())
}
