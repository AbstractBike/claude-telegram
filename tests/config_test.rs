use claude_chat::config::Config;

#[test]
fn loads_config_from_toml_string() {
    let toml = r#"
[matrix]
homeserver = "http://127.0.0.1:8008"
user = "@claude-bot:matrix.pin"
password_file = "/run/secrets/matrix-password"

[auth]
default_allowed_users = ["@digger:matrix.abstract.bike"]

[rooms.control]
room_id = "!abc123:matrix.pin"

[rooms.agents.nixos]
room_id = "!def456:matrix.pin"
work_dir = "/home/digger/git/nixos"
store_dir = "/home/digger/.agent-store/nixos"
timeout_secs = 300

[inter_agent]
timeout_secs = 180
max_depth = 3

[observability]
metrics_port = 9090
skywalking_endpoint = "http://192.168.0.4:11800"
"#;
    let config = Config::from_str(toml).unwrap();
    assert_eq!(config.matrix.user, "@claude-bot:matrix.pin");
    assert_eq!(config.auth.default_allowed_users, vec!["@digger:matrix.abstract.bike"]);
    assert_eq!(config.rooms.agents.len(), 1);
    assert!(config.rooms.agents.contains_key("nixos"));
    assert_eq!(config.rooms.agents["nixos"].timeout_secs, Some(300));
    assert_eq!(config.rooms.agents["nixos"].timeout(), 300);
    assert_eq!(config.inter_agent.max_depth, 3);
    assert_eq!(config.observability.metrics_port, 9090);
}

#[test]
fn agent_inherits_default_allowed_users() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = ["@admin:localhost"]

[rooms.control]
room_id = "!ctrl:localhost"

[rooms.agents.myrepo]
room_id = "!repo:localhost"
work_dir = "/tmp/myrepo"
store_dir = "/tmp/.store/myrepo"
"#;
    let config = Config::from_str(toml).unwrap();
    let agent = &config.rooms.agents["myrepo"];
    let effective = agent.effective_allowed_users(&config.auth.default_allowed_users);
    assert_eq!(effective, &["@admin:localhost"]);
}

#[test]
fn agent_overrides_allowed_users() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = ["@admin:localhost"]

[rooms.control]
room_id = "!ctrl:localhost"

[rooms.agents.collab]
room_id = "!collab:localhost"
work_dir = "/tmp/collab"
store_dir = "/tmp/.store/collab"
allowed_users = ["@admin:localhost", "@alice:localhost"]
"#;
    let config = Config::from_str(toml).unwrap();
    let agent = &config.rooms.agents["collab"];
    let effective = agent.effective_allowed_users(&config.auth.default_allowed_users);
    assert!(effective.contains(&"@alice:localhost".to_string()));
    assert_eq!(effective.len(), 2);
}

#[test]
fn defaults_for_optional_fields() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = []

[rooms.control]
room_id = "!ctrl:localhost"
"#;
    let config = Config::from_str(toml).unwrap();
    assert_eq!(config.inter_agent.timeout_secs, 180);
    assert_eq!(config.inter_agent.max_depth, 3);
    assert_eq!(config.observability.metrics_port, 9090);
    assert!(config.rooms.agents.is_empty());
}

#[test]
fn default_path_is_not_empty() {
    let path = Config::default_path();
    assert!(!path.as_os_str().is_empty());
    assert!(path.to_str().unwrap().contains("claude-chat"));
}

#[test]
fn agent_encrypt_defaults_to_true() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = []

[rooms.control]
room_id = "!ctrl:localhost"

[rooms.agents.myagent]
room_id = "!agent:localhost"
work_dir = "/tmp/work"
store_dir = "/tmp/store"
"#;
    let config = Config::from_str(toml).unwrap();
    assert!(config.rooms.agents["myagent"].encrypt);
}

#[test]
fn agent_encrypt_can_be_disabled() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = []

[rooms.control]
room_id = "!ctrl:localhost"

[rooms.agents.myagent]
room_id = "!agent:localhost"
work_dir = "/tmp/work"
store_dir = "/tmp/store"
encrypt = false
"#;
    let config = Config::from_str(toml).unwrap();
    assert!(!config.rooms.agents["myagent"].encrypt);
}

#[test]
fn vault_config_parsed() {
    let toml = r#"
[matrix]
homeserver = "http://localhost"
user = "@bot:localhost"
password_file = "/tmp/pw"

[auth]
default_allowed_users = []

[rooms.control]
room_id = "!ctrl:localhost"

[vault]
root = "/home/digger/.config/claude-chat/vault"
"#;
    let config = Config::from_str(toml).unwrap();
    assert_eq!(config.vault.as_ref().unwrap().root, "/home/digger/.config/claude-chat/vault");
}
