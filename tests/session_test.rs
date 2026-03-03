use claude_chat::session::claude::ClaudeSession;

#[test]
fn session_id_from_room_alias() {
    assert_eq!(
        ClaudeSession::session_id_from_alias("#nixos-agent:matrix.pin"),
        "nixos-agent"
    );
    assert_eq!(
        ClaudeSession::session_id_from_alias("nixos-agent:matrix.pin"),
        "nixos-agent"
    );
    assert_eq!(
        ClaudeSession::session_id_from_alias("nixos-agent"),
        "nixos-agent"
    );
}

#[tokio::test]
async fn session_captures_stdout() {
    let session = ClaudeSession::new_with_bin(
        "test-session".to_string(),
        "/tmp".to_string(),
        120,
        "echo",
    );
    let result = session.send_raw("hello world").await.unwrap();
    assert!(!result.is_empty());
    assert!(result.contains("hello"));
}

#[tokio::test]
async fn sandboxed_session_builds_command() {
    let session = ClaudeSession::new_sandboxed(
        "test-session".to_string(),
        "/tmp/work".to_string(),
        "/tmp/store".to_string(),
        5,
        "echo",
    );
    let _ = session.build_command("hello");
    // Just verify it builds without panic
}

#[test]
fn new_session_defaults_to_claude_binary() {
    let session = ClaudeSession::new("test".to_string(), "/tmp".to_string(), 120);
    assert_eq!(session.timeout_secs, 120);
    assert!(session.store_dir.is_none());
}

#[test]
fn sandboxed_session_command_includes_bwrap() {
    let session = ClaudeSession::new_sandboxed(
        "test-agent".to_string(),
        "/tmp/work".to_string(),
        "/tmp/store".to_string(),
        120,
        "echo",
    );

    let cmd = session.build_command("hello world");
    let program = format!("{:?}", cmd);

    // The command should use bwrap as the program
    assert!(
        program.contains("bwrap"),
        "sandboxed session should use bwrap, got: {}",
        program
    );
}

#[test]
fn unsandboxed_session_command_uses_claude_directly() {
    let session = ClaudeSession::new_with_bin(
        "test-agent".to_string(),
        "/tmp".to_string(),
        120,
        "echo",
    );

    let cmd = session.build_command("hello world");
    let program = format!("{:?}", cmd);

    // The command should use echo directly (not bwrap)
    assert!(
        !program.contains("bwrap"),
        "unsandboxed session should not use bwrap, got: {}",
        program
    );
    assert!(
        program.contains("echo"),
        "should use the configured binary"
    );
}
