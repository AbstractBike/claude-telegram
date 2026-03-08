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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::config::Config;
use crate::matrix::control::parse_control_command;
use crate::matrix::handler::{classify_message, check_auth, AuthResult, MessageSource};
use crate::matrix::sender::send_text;
use crate::session::claude::ClaudeSession;
use crate::temporal::client::TemporalDispatcher;
use crate::temporal::workflow::IncomingMessage;

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

/// Run the Matrix sync loop forever. Reconnects on errors with exponential backoff.
/// This function only returns if an unrecoverable error occurs during initial setup.
pub async fn run_sync(
    client: Client,
    config: Arc<Config>,
    temporal: Option<Arc<TemporalDispatcher>>,
) -> Result<()> {
    tracing::info!("starting Matrix sync loop");

    // Gate to skip messages received during initial sync
    let ready = Arc::new(AtomicBool::new(false));

    let own_user_id = config.matrix.user.clone();

    client.add_event_handler({
        let config = config.clone();
        let own_user = own_user_id.clone();
        let ready = ready.clone();
        let temporal = temporal.clone();
        move |event: SyncRoomMessageEvent, room: Room| {
            let config = config.clone();
            let own_user = own_user.clone();
            let ready = ready.clone();
            let temporal = temporal.clone();
            async move {
                // Skip messages until initial sync completes
                if !ready.load(Ordering::Relaxed) {
                    return;
                }

                let event = match event {
                    SyncRoomMessageEvent::Original(e) => e,
                    _ => return,
                };
                if let Err(e) = handle_room_message(event, room, &config, &own_user, temporal.as_deref()).await {
                    tracing::error!(error = %e, "failed to handle message");
                }
            }
        }
    });

    // Initial sync to establish position — old messages are skipped by the ready gate
    let response = client.sync_once(SyncSettings::default()).await?;
    tracing::info!("initial sync complete, listening for new messages");

    // Now enable message processing
    ready.store(true, Ordering::Relaxed);

    // Sync loop with reconnection on transient errors
    let mut next_batch = response.next_batch;
    let mut backoff_secs = 1u64;
    let max_backoff_secs = 60u64;

    loop {
        let settings = SyncSettings::default().token(next_batch.clone());
        let start = Instant::now();

        match client.sync(settings).await {
            Ok(_) => {
                // sync() normally runs forever; if it returns Ok, something unexpected happened
                tracing::warn!("sync loop returned unexpectedly, restarting");
                backoff_secs = 1;
            }
            Err(e) => {
                let elapsed = start.elapsed().as_secs_f64();
                histogram!("claude_chat_matrix_sync_duration_seconds").record(elapsed);
                counter!("claude_chat_matrix_sync_errors_total", "error" => format!("{e}"))
                    .increment(1);

                tracing::error!(
                    error = %e,
                    backoff_secs,
                    "sync loop error, will retry"
                );

                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(max_backoff_secs);

                // Try to get a fresh sync token
                match client.sync_once(SyncSettings::default()).await {
                    Ok(resp) => {
                        next_batch = resp.next_batch;
                        tracing::info!("re-synced successfully, resuming");
                        backoff_secs = 1;
                    }
                    Err(resync_err) => {
                        tracing::error!(error = %resync_err, "re-sync failed, will retry");
                    }
                }
            }
        }
    }
}

async fn handle_room_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: &Config,
    own_user: &str,
    temporal: Option<&TemporalDispatcher>,
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
            if let Err(e) = send_text(&room, "Access denied.").await {
                tracing::error!(error = %e, "failed to send denial message");
            }
            return Ok(());
        }
    }

    // Classify message
    let source = classify_message(&body, config.inter_agent.max_depth);

    // Pre-extract depth from agent messages before consuming source in the match
    let agent_depth: u8 = if let MessageSource::AgentMessage { depth, .. } = &source {
        *depth
    } else {
        0
    };

    match source {
        MessageSource::ControlCommand(cmd) => {
            tracing::info!(agent = %agent_name, cmd = %cmd, "control command");
            let reply = handle_agent_command(&cmd, &agent_name);
            if let Err(e) = send_text(&room, &reply).await {
                tracing::error!(error = %e, agent = %agent_name, "failed to send command reply");
            }
        }
        MessageSource::DepthExceeded { from, depth } => {
            tracing::warn!(agent = %agent_name, from = %from, depth, "inter-agent depth exceeded");
            let msg = format!("Inter-agent depth limit exceeded (depth={depth}, from={from})");
            if let Err(e) = send_text(&room, &msg).await {
                tracing::error!(error = %e, "failed to send depth-exceeded message");
            }
        }
        MessageSource::UserMessage(text) | MessageSource::AgentMessage { text, .. } => {
            tracing::info!(agent = %agent_name, "handling message");

            if let Some(dispatcher) = temporal {
                // Temporal path: ensure workflow is running, then signal it
                let wf_input = crate::temporal::client::TemporalDispatcher::build_workflow_input(
                    &agent_name, agent_config, config,
                );
                if let Err(e) = dispatcher.ensure_running(&agent_name, wf_input).await {
                    tracing::warn!(agent = %agent_name, error = %e, "failed to ensure workflow running");
                }
                let event_id = event.event_id.to_string();
                let msg = IncomingMessage {
                    text,
                    from: sender.clone(),
                    event_id,
                    depth: agent_depth,
                };
                if let Err(e) = dispatcher.send_message(&agent_name, msg).await {
                    tracing::error!(error = %e, agent = %agent_name, "failed to signal workflow");
                    if let Err(e2) = send_text(&room, &format!("(failed to dispatch: {e})")).await {
                        tracing::error!(error = %e2, "failed to send error message");
                    }
                }
                // Fire-and-forget: workflow will send the response
            } else {
                // Direct path (no Temporal): run Claude inline
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
                )
                .with_claude_home(config.claude_home.clone());

                let response = match session.send_raw(&text).await {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!(error = %e, agent = %agent_name, "Claude session error");
                        counter!("claude_chat_session_errors_total",
                            "agent" => agent_name.clone()).increment(1);
                        format!("(error: {e})")
                    }
                };

                if let Err(e) = send_text(&room, &response).await {
                    tracing::error!(error = %e, agent = %agent_name, "failed to send response");
                }

                tracing::info!(agent = %agent_name, response_len = response.len(), "response sent");
            }
        }
    }

    Ok(())
}

const HELP_TEXT: &str = "\
Available commands (use / or !):

/help              — show this message
/status            — platform status
/list              — list configured agents
/reset <agent>     — reset agent session
/spawn <agent>     — spawn a new agent
/kill <agent>      — stop a running agent
/audit [agent]     — show audit log
/grant <agent> <secret>   — grant secret access
/revoke <agent> <secret>  — revoke secret access

Send any other message to chat with the agent.";

fn handle_agent_command(cmd: &str, agent_name: &str) -> String {
    use crate::matrix::control::ControlCommand;

    match parse_control_command(cmd) {
        Some(ControlCommand::Help) | None if cmd == "/" || cmd == "!" => HELP_TEXT.to_string(),
        Some(ControlCommand::Help) => HELP_TEXT.to_string(),
        Some(ControlCommand::Status) => format!("Agent **{}** is running.", agent_name),
        Some(ControlCommand::List) => format!("Current agent: **{}**\nFor full agent list use the control room.", agent_name),
        Some(_) => format!(
            "Command `{cmd}` must be sent to the control room.\n\n{}",
            HELP_TEXT
        ),
        None => format!("Unknown command: `{cmd}`\n\n{}", HELP_TEXT),
    }
}
