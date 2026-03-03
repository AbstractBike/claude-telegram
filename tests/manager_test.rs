use claude_chat::session::manager::{AgentState, HistoryEntry};
use tempfile::TempDir;

#[tokio::test]
async fn agent_state_persists_last_event() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().to_str().unwrap();

    let mut state = AgentState::load_or_create("test-agent", store).await.unwrap();
    assert!(state.last_processed_event.is_none());
    assert_eq!(state.session_id, "test-agent");

    state.last_processed_event = Some("$event_abc123".to_string());
    state.save().await.unwrap();

    // Reload from disk
    let loaded = AgentState::load_or_create("test-agent", store).await.unwrap();
    assert_eq!(loaded.last_processed_event.as_deref(), Some("$event_abc123"));
    assert_eq!(loaded.session_id, "test-agent");
}

#[tokio::test]
async fn agent_state_history_appends() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().to_str().unwrap();
    let state = AgentState::load_or_create("test-agent", store).await.unwrap();

    let entry = HistoryEntry {
        event_id: "$abc".to_string(),
        ts: chrono::Utc::now(),
        from: "@digger:matrix.pin".to_string(),
        text: "hello agent".to_string(),
        response_event: Some("$def".to_string()),
        duration_ms: 1234,
        exit: "success".to_string(),
    };
    state.append_history(&entry).await.unwrap();

    let history_path = dir.path().join("history.jsonl");
    assert!(history_path.exists());
    let content = std::fs::read_to_string(history_path).unwrap();
    assert!(content.contains("$abc"));
    assert!(content.contains("hello agent"));
    assert!(content.contains("1234"));
}

#[tokio::test]
async fn agent_state_creates_dir_if_missing() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join("nested/deep/store");
    let store_str = store.to_str().unwrap();

    let state = AgentState::load_or_create("test", store_str).await.unwrap();
    assert!(store.exists());

    state.save().await.unwrap();
    assert!(store.join("state.toml").exists());
}

#[tokio::test]
async fn history_appends_multiple_entries() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().to_str().unwrap();
    let state = AgentState::load_or_create("test-agent", store).await.unwrap();

    for i in 0..3 {
        let entry = HistoryEntry {
            event_id: format!("$event_{i}"),
            ts: chrono::Utc::now(),
            from: "@user:host".to_string(),
            text: format!("message {i}"),
            response_event: None,
            duration_ms: i * 100,
            exit: "success".to_string(),
        };
        state.append_history(&entry).await.unwrap();
    }

    let content = std::fs::read_to_string(dir.path().join("history.jsonl")).unwrap();
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 history entries");
}
