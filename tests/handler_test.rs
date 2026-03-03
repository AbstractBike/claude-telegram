use claude_chat::matrix::handler::{AuthResult, MessageSource, check_auth, classify_message};

#[test]
fn classifies_control_room_command() {
    let result = classify_message("!list", 3);
    assert!(matches!(result, MessageSource::ControlCommand(_)));
}

#[test]
fn classifies_agent_room_message() {
    let result = classify_message("update the flake", 3);
    assert!(matches!(result, MessageSource::UserMessage(_)));
}

#[test]
fn classifies_inter_agent_message() {
    let result = classify_message("[from:nixos, depth:1] what version?", 3);
    assert!(matches!(result, MessageSource::AgentMessage { .. }));

    if let MessageSource::AgentMessage { from, depth, text } = result {
        assert_eq!(from, "nixos");
        assert_eq!(depth, 1);
        assert_eq!(text, "what version?");
    }
}

#[test]
fn classifies_depth_exceeded() {
    let result = classify_message("[from:nixos, depth:3] recurse", 3);
    assert!(matches!(result, MessageSource::DepthExceeded { .. }));
}

#[test]
fn auth_allows_permitted_user() {
    let allowed = vec!["@alice:host".to_string()];
    assert!(matches!(
        check_auth("@alice:host", &allowed),
        AuthResult::Allowed
    ));
}

#[test]
fn auth_denies_unknown_user() {
    let allowed = vec!["@alice:host".to_string()];
    assert!(matches!(
        check_auth("@eve:host", &allowed),
        AuthResult::Denied
    ));
}
