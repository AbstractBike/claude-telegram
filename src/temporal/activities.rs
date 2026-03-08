use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use temporalio_macros::activities;
use temporalio_sdk::activities::{ActivityContext, ActivityError};
use matrix_sdk::Client;

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunClaudeInput {
    pub agent_name: String,
    pub session_id: String,
    pub work_dir: String,
    pub store_dir: String,
    pub timeout_secs: u64,
    pub text: String,
    pub event_id: String,
    pub from: String,
    pub claude_bin: String,
    pub claude_home: Option<String>,
    pub vault_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunClaudeOutput {
    pub response: String,
    pub duration_ms: u64,
    pub exit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SendMatrixInput {
    pub room_id: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResetSessionInput {
    pub store_dir: String,
    pub session_id: String,
}

pub struct ClaudeChatActivities {
    pub matrix_client: Arc<Client>,
    pub config: Arc<Config>,
}

#[activities]
impl ClaudeChatActivities {
    #[activity]
    pub async fn run_claude(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: RunClaudeInput,
    ) -> Result<RunClaudeOutput, ActivityError> {
        // Write MCP config if vault is configured
        if let Some(ref vault_root) = input.vault_root {
            let bin = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "claude-chat".to_string());
            let config = serde_json::json!({
                "mcpServers": {
                    "vault": {
                        "command": bin,
                        "args": ["mcp-vault", "--vault-root", vault_root, "--agent", &input.agent_name]
                    }
                }
            });
            let path = std::path::Path::new(&input.work_dir).join(".mcp.json");
            let _ = tokio::fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).await;
        }

        let session = crate::session::claude::ClaudeSession::new_sandboxed(
            input.session_id.clone(),
            input.work_dir.clone(),
            input.store_dir.clone(),
            input.timeout_secs,
            input.claude_bin.clone(),
        )
        .with_claude_home(input.claude_home.clone());

        let start = Instant::now();
        let (response, exit) = match session.send_raw(&input.text).await {
            Ok(r) => (r, "success".to_string()),
            Err(e) => (format!("(error: {e})"), "error".to_string()),
        };
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(RunClaudeOutput { response, duration_ms, exit })
    }

    #[activity]
    pub async fn send_matrix_message(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: SendMatrixInput,
    ) -> Result<(), ActivityError> {
        use matrix_sdk::ruma::RoomId;

        let room_id = RoomId::parse(&input.room_id)
            .map_err(|e| ActivityError::NonRetryable(
                anyhow::anyhow!("invalid room_id: {e}").into()
            ))?;

        let room = self.matrix_client.get_room(&room_id)
            .ok_or_else(|| ActivityError::Retryable {
                source: anyhow::anyhow!("room {} not found", input.room_id).into(),
                explicit_delay: None,
            })?;

        crate::matrix::sender::send_text(&room, &input.text).await
            .map_err(|e| ActivityError::Retryable {
                source: e.into(),
                explicit_delay: None,
            })?;

        Ok(())
    }

    #[activity]
    pub async fn reset_session(
        self: Arc<Self>,
        _ctx: ActivityContext,
        input: ResetSessionInput,
    ) -> Result<(), ActivityError> {
        // Delete Claude session files from store_dir so --resume starts fresh
        let store = std::path::Path::new(&input.store_dir);
        let sessions_dir = store.join("projects").join(&input.session_id);
        if sessions_dir.exists() {
            tokio::fs::remove_dir_all(&sessions_dir).await
                .map_err(|e| ActivityError::Retryable {
                    source: e.into(),
                    explicit_delay: None,
                })?;
            tracing::info!(session = %input.session_id, "session reset");
        }
        Ok(())
    }
}
