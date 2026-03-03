use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize, Default)]
struct AgentStatePersisted {
    session_id: String,
    last_processed_event: Option<String>,
}

#[derive(Debug)]
pub struct AgentState {
    pub session_id: String,
    pub store_dir: PathBuf,
    pub last_processed_event: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub event_id: String,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub from: String,
    pub text: String,
    pub response_event: Option<String>,
    pub duration_ms: u64,
    pub exit: String,
}

impl AgentState {
    pub async fn load_or_create(agent_name: &str, store_dir: &str) -> Result<Self> {
        let store = PathBuf::from(store_dir);
        fs::create_dir_all(&store).await?;

        let state_file = store.join("state.toml");
        let persisted = if state_file.exists() {
            let content = fs::read_to_string(&state_file).await?;
            toml::from_str::<AgentStatePersisted>(&content).unwrap_or_default()
        } else {
            AgentStatePersisted {
                session_id: agent_name.to_string(),
                last_processed_event: None,
            }
        };

        Ok(Self {
            session_id: persisted.session_id,
            store_dir: store,
            last_processed_event: persisted.last_processed_event,
        })
    }

    pub async fn save(&self) -> Result<()> {
        let persisted = AgentStatePersisted {
            session_id: self.session_id.clone(),
            last_processed_event: self.last_processed_event.clone(),
        };
        let content = toml::to_string_pretty(&persisted)?;
        fs::write(self.store_dir.join("state.toml"), content).await?;
        Ok(())
    }

    pub async fn append_history(&self, entry: &HistoryEntry) -> Result<()> {
        let path = self.store_dir.join("history.jsonl");
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let line = serde_json::to_string(entry)? + "\n";
        file.write_all(line.as_bytes()).await?;
        file.flush().await?;

        tracing::info!(
            service = "claude-chat",
            event = "message_processed",
            agent = %self.session_id,
            event_id = %entry.event_id,
            from = %entry.from,
            duration_ms = entry.duration_ms,
            exit = %entry.exit,
            "agent processed message"
        );

        Ok(())
    }
}
