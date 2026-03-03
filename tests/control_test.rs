use claude_chat::matrix::control::{parse_control_command, ControlCommand};

#[test]
fn parses_list_command() {
    assert!(matches!(parse_control_command("!list"), Some(ControlCommand::List)));
}

#[test]
fn parses_status_command() {
    assert!(matches!(parse_control_command("!status"), Some(ControlCommand::Status)));
}

#[test]
fn parses_reset_with_agent() {
    let cmd = parse_control_command("!reset nixos");
    assert!(matches!(cmd, Some(ControlCommand::Reset(ref name)) if name == "nixos"));
}

#[test]
fn parses_spawn_command() {
    let cmd = parse_control_command("!spawn home");
    assert!(matches!(cmd, Some(ControlCommand::Spawn(ref name)) if name == "home"));
}

#[test]
fn parses_kill_command() {
    let cmd = parse_control_command("!kill nixos");
    assert!(matches!(cmd, Some(ControlCommand::Kill(ref name)) if name == "nixos"));
}

#[test]
fn parses_audit_no_agent() {
    let cmd = parse_control_command("!audit");
    assert!(matches!(cmd, Some(ControlCommand::Audit(None))));
}

#[test]
fn parses_audit_with_agent() {
    let cmd = parse_control_command("!audit nixos");
    assert!(matches!(cmd, Some(ControlCommand::Audit(Some(ref name))) if name == "nixos"));
}

#[test]
fn parses_grant_command() {
    let cmd = parse_control_command("!grant nixos github-token");
    assert!(matches!(cmd, Some(ControlCommand::Grant { ref agent, ref secret })
        if agent == "nixos" && secret == "github-token"));
}

#[test]
fn parses_revoke_command() {
    let cmd = parse_control_command("!revoke nixos npm-token");
    assert!(matches!(cmd, Some(ControlCommand::Revoke { ref agent, ref secret })
        if agent == "nixos" && secret == "npm-token"));
}

#[test]
fn parses_help_command() {
    assert!(matches!(parse_control_command("!help"), Some(ControlCommand::Help)));
}

#[test]
fn returns_none_for_unknown_command() {
    assert!(parse_control_command("!foobar").is_none());
}

#[test]
fn returns_none_for_non_command() {
    assert!(parse_control_command("hello world").is_none());
}
