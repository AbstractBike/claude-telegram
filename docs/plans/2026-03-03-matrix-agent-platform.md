# Matrix Agent Platform Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite claude-chat from Python/Telegram to a Rust/Matrix multi-agent platform with bubblewrap sandboxing, persistent Claude sessions, encrypted secrets via MCP, and full observability.

**Architecture:** Single Rust binary (monolith) using `matrix-sdk` for Matrix, `tokio` for async, `bwrap` for sandboxing, and an embedded MCP server for secrets. Matrix room timelines serve as the message queue.

**Tech Stack:** Rust 2021, matrix-sdk 0.7, tokio, serde/toml, tracing (JSON), metrics-exporter-prometheus, skywalking, age (rage), rmcp, axum, anyhow, bubblewrap.

---

## Phase 1: Rust Project Scaffolding

### Task 1: Initialize Rust project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`

**Step 1: Initialize cargo project**

```bash
cd /home/digger/git/claude-chat
cargo init --name claude-chat
```

Expected: `src/main.rs` created, `Cargo.toml` created.

**Step 2: Replace `Cargo.toml` with full dependencies**

```toml
[package]
name = "claude-chat"
version = "0.5.0"
edition = "2021"

[[bin]]
name = "claude-chat"
path = "src/main.rs"

[dependencies]
# Matrix
matrix-sdk = { version = "0.7", features = ["rustls-tls"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging / tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }

# Metrics
metrics = "0.23"
metrics-exporter-prometheus = "0.15"

# HTTP (metrics endpoint)
axum = "0.7"

# Age encryption
age = { version = "0.10", features = ["cli-common"] }

# MCP server
rmcp = { version = "0.1", features = ["server", "transport-io"] }

# Utilities
chrono = { version = "0.4", features = ["serde"] }
dirs = "5"
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
```

**Step 3: Create module skeleton in `src/main.rs`**

```rust
mod config;
mod matrix;
mod session;
mod sandbox;
mod agent;
mod secrets;
mod observability;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    observability::logging::init();
    tracing::info!("claude-chat starting");
    Ok(())
}
```

**Step 4: Create empty module files**

```bash
mkdir -p src/matrix src/session src/sandbox src/agent src/secrets src/observability
touch src/config.rs
touch src/matrix/mod.rs src/matrix/client.rs src/matrix/handler.rs src/matrix/sender.rs
touch src/session/mod.rs src/session/manager.rs src/session/claude.rs
touch src/sandbox/mod.rs src/sandbox/bwrap.rs
touch src/agent/mod.rs src/agent/tool.rs
touch src/secrets/mod.rs src/secrets/mcp_server.rs src/secrets/vault.rs
touch src/observability/mod.rs src/observability/metrics.rs src/observability/logging.rs src/observability/tracing.rs
```

**Step 5: Verify it compiles**

```bash
cargo check
```

Expected: no errors (only "unused" warnings OK).

**Step 6: Commit**

```bash
git add Cargo.toml src/
git commit -m "chore: initialize Rust project structure for Matrix agent platform"
```

---

## Phase 2: Observability Foundation

> Build this first — everything else emits metrics/logs from day one.

### Task 2: Structured JSON logging

**Files:**
- Modify: `src/observability/logging.rs`
- Modify: `src/observability/mod.rs`

**Step 1: Write failing test**

```rust
// src/observability/logging.rs — add at bottom
#[cfg(test)]
mod tests {
    #[test]
    fn init_does_not_panic() {
        // init() must be idempotent — calling twice must not panic
        super::init();
        super::init();
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test observability::logging::tests::init_does_not_panic 2>&1 | head -20
```

Expected: FAIL — `init` not defined.

**Step 3: Implement `init()`**

```rust
// src/observability/logging.rs
use tracing_subscriber::{fmt, EnvFilter};

pub fn init() {
    let _ = fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("claude_chat=info".parse().unwrap()))
        .with_current_span(true)
        .try_init();
}
```

```rust
// src/observability/mod.rs
pub mod logging;
pub mod metrics;
pub mod tracing;
```

**Step 4: Run test to verify it passes**

```bash
cargo test observability::logging
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/observability/
git commit -m "feat: add JSON structured logging via tracing-subscriber"
```

---

### Task 3: Prometheus metrics endpoint

**Files:**
- Modify: `src/observability/metrics.rs`

**Step 1: Write failing test**

```rust
// src/observability/metrics.rs
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn metrics_server_binds() {
        let addr = "127.0.0.1:0".parse().unwrap();
        let handle = super::start_metrics_server(addr).await;
        assert!(handle.is_ok(), "metrics server failed to bind: {:?}", handle);
    }
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test observability::metrics
```

Expected: FAIL — `start_metrics_server` not defined.

**Step 3: Implement metrics server**

```rust
// src/observability/metrics.rs
use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;

pub async fn start_metrics_server(addr: SocketAddr) -> Result<()> {
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()?;
    tracing::info!(%addr, "metrics server started");
    Ok(())
}

/// Register all application metrics (call once at startup)
pub fn register_metrics() {
    use metrics::{counter, gauge, histogram, describe_counter, describe_gauge, describe_histogram, Unit};

    describe_counter!("claude_chat_matrix_messages_received_total", "Matrix messages received");
    describe_counter!("claude_chat_matrix_messages_sent_total", "Matrix messages sent");
    describe_counter!("claude_chat_auth_checks_total", "Auth checks performed");
    describe_counter!("claude_chat_session_started_total", "Claude sessions started");
    describe_counter!("claude_chat_session_completed_total", "Claude sessions completed");
    describe_gauge!("claude_chat_session_active", Unit::Count, "Active Claude sessions");
    describe_histogram!("claude_chat_session_duration_seconds", Unit::Seconds, "Session duration");
    describe_counter!("claude_chat_command_executed_total", "Subprocess commands executed");
    describe_histogram!("claude_chat_command_duration_seconds", Unit::Seconds, "Command duration");
    describe_counter!("claude_chat_bwrap_spawns_total", "Bubblewrap sandbox spawns");
    describe_counter!("claude_chat_bwrap_failures_total", "Bubblewrap failures");
    describe_counter!("claude_chat_mcp_secret_requests_total", "MCP secret requests");
    describe_counter!("claude_chat_agent_messages_sent_total", "Inter-agent messages sent");
    describe_histogram!("claude_chat_agent_roundtrip_seconds", Unit::Seconds, "Inter-agent roundtrip");
    describe_counter!("claude_chat_control_commands_total", "Control room commands");
    describe_gauge!("claude_chat_uptime_seconds", Unit::Seconds, "Bot uptime");
    describe_gauge!("claude_chat_rooms_configured", Unit::Count, "Rooms configured");
    describe_gauge!("claude_chat_rooms_active", Unit::Count, "Rooms active");
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test observability::metrics
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/observability/metrics.rs
git commit -m "feat: add Prometheus metrics endpoint"
```

---

## Phase 3: Configuration

### Task 4: Config struct with TOML loading

**Files:**
- Create: `src/config.rs`
- Create: `tests/config_test.rs`
- Create: `tests/fixtures/config.toml`

**Step 1: Write failing test**

```rust
// tests/config_test.rs
use claude_chat::config::Config;

#[test]
fn loads_config_from_toml() {
    let toml = r#"
[matrix]
homeserver = "http://192.168.0.4:8008"
user = "@claude-bot:matrix.pin"
password_file = "/run/secrets/matrix-password"

[auth]
default_allowed_users = ["@digger:matrix.pin"]

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
    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.matrix.user, "@claude-bot:matrix.pin");
    assert_eq!(config.auth.default_allowed_users, vec!["@digger:matrix.pin"]);
    assert_eq!(config.rooms.agents.len(), 1);
    assert!(config.rooms.agents.contains_key("nixos"));
    assert_eq!(config.rooms.agents["nixos"].timeout_secs, Some(300));
    assert_eq!(config.inter_agent.max_depth, 3);
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
    let config: Config = toml::from_str(toml).unwrap();
    let agent = &config.rooms.agents["myrepo"];
    let effective = agent.effective_allowed_users(&config.auth.default_allowed_users);
    assert_eq!(effective, vec!["@admin:localhost"]);
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
    let config: Config = toml::from_str(toml).unwrap();
    let agent = &config.rooms.agents["collab"];
    let effective = agent.effective_allowed_users(&config.auth.default_allowed_users);
    assert!(effective.contains(&"@alice:localhost".to_string()));
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test config 2>&1 | head -20
```

Expected: FAIL — module not found.

**Step 3: Implement Config**

```rust
// src/config.rs
use serde::Deserialize;
use std::collections::HashMap;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub auth: AuthConfig,
    pub rooms: RoomsConfig,
    #[serde(default)]
    pub inter_agent: InterAgentConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub user: String,
    pub password_file: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub default_allowed_users: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoomsConfig {
    pub control: ControlRoomConfig,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlRoomConfig {
    pub room_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    pub room_id: String,
    pub work_dir: String,
    pub store_dir: String,
    pub timeout_secs: Option<u64>,
    pub allowed_users: Option<Vec<String>>,
}

impl AgentConfig {
    pub fn timeout(&self) -> u64 {
        self.timeout_secs.unwrap_or(120)
    }

    pub fn effective_allowed_users<'a>(&'a self, defaults: &'a Vec<String>) -> &'a Vec<String> {
        self.allowed_users.as_ref().unwrap_or(defaults)
    }

    /// Derive deterministic session_id from agent name
    pub fn session_id(name: &str) -> String {
        name.to_string()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct InterAgentConfig {
    pub timeout_secs: u64,
    pub max_depth: u8,
}

impl Default for InterAgentConfig {
    fn default() -> Self {
        Self { timeout_secs: 180, max_depth: 3 }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ObservabilityConfig {
    pub metrics_port: u16,
    pub skywalking_endpoint: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            metrics_port: 9090,
            skywalking_endpoint: "http://192.168.0.4:11800".to_string(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing config from {}", path.display()))
    }

    pub fn default_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("claude-chat")
            .join("config.toml")
    }

    pub fn matrix_password(&self) -> Result<String> {
        std::fs::read_to_string(&self.matrix.password_file)
            .map(|s| s.trim().to_string())
            .with_context(|| format!("reading password from {}", self.matrix.password_file))
    }
}

// Re-export so tests can use claude_chat::config::Config
pub use self::Config as Config;
```

**Step 4: Add `pub mod config;` to `src/lib.rs`**

```rust
// src/lib.rs
pub mod config;
pub mod observability;
```

**Step 5: Run tests to verify they pass**

```bash
cargo test config
```

Expected: 3 tests PASS.

**Step 6: Commit**

```bash
git add src/config.rs src/lib.rs tests/config_test.rs
git commit -m "feat: add TOML config with per-agent auth inheritance"
```

---

## Phase 4: Bubblewrap Sandbox

### Task 5: BwrapBuilder

**Files:**
- Modify: `src/sandbox/bwrap.rs`
- Modify: `src/sandbox/mod.rs`
- Create: `tests/sandbox_test.rs`

**Step 1: Write failing test**

```rust
// tests/sandbox_test.rs
use claude_chat::sandbox::bwrap::BwrapBuilder;

#[test]
fn bwrap_command_contains_required_args() {
    let cmd = BwrapBuilder::new("/home/user/git/nixos", "/home/user/.agent-store/nixos")
        .build_args();

    // Must include ro-bind for /nix
    assert!(cmd.iter().any(|a| a == "/nix"), "missing /nix ro-bind");
    // Must include workdir bind
    assert!(cmd.iter().any(|a| a == "/home/user/git/nixos"), "missing workdir bind");
    // Must include store bind
    assert!(cmd.iter().any(|a| a == "/home/user/.agent-store/nixos"), "missing store bind");
    // Must unshare namespaces
    assert!(cmd.contains(&"--unshare-all".to_string()), "missing --unshare-all");
    // Must share network
    assert!(cmd.contains(&"--share-net".to_string()), "missing --share-net");
    // Must die with parent
    assert!(cmd.contains(&"--die-with-parent".to_string()), "missing --die-with-parent");
}

#[test]
fn bwrap_does_not_expose_home_dir() {
    let cmd = BwrapBuilder::new("/home/user/git/nixos", "/home/user/.agent-store/nixos")
        .build_args();

    let args_str = cmd.join(" ");
    // Must NOT blindly bind entire home
    assert!(!args_str.contains("--bind /home/user /home/user"), "exposes entire home dir");
    // Must NOT expose .ssh
    assert!(!args_str.contains(".ssh"), "exposes .ssh directory");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test sandbox
```

Expected: FAIL — module not found.

**Step 3: Implement BwrapBuilder**

```rust
// src/sandbox/bwrap.rs
use std::path::Path;
use tokio::process::Command;

pub struct BwrapBuilder {
    work_dir: String,
    store_dir: String,
}

impl BwrapBuilder {
    pub fn new(work_dir: impl Into<String>, store_dir: impl Into<String>) -> Self {
        Self {
            work_dir: work_dir.into(),
            store_dir: store_dir.into(),
        }
    }

    /// Returns the bwrap arguments (not including "bwrap" itself or the command to run)
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![
            // Read-only system binds
            "--ro-bind".into(), "/nix".into(), "/nix".into(),
            "--ro-bind".into(), "/usr".into(), "/usr".into(),
            "--ro-bind".into(), "/etc/resolv.conf".into(), "/etc/resolv.conf".into(),
            // Minimal proc/dev
            "--proc".into(), "/proc".into(),
            "--dev".into(), "/dev".into(),
            // Ephemeral /tmp
            "--tmpfs".into(), "/tmp".into(),
            // Agent workdir (read-write)
            "--bind".into(), self.work_dir.clone(), self.work_dir.clone(),
            // Agent store (read-write, persistent installs)
            "--bind".into(), self.store_dir.clone(), self.store_dir.clone(),
            // Isolation
            "--unshare-all".into(),
            "--share-net".into(),      // network allowed (monitoring added later)
            "--die-with-parent".into(),
        ];
        args
    }

    /// Wrap a tokio Command with bwrap sandboxing
    pub fn wrap_command(&self, program: &str, program_args: &[&str]) -> Command {
        let mut cmd = Command::new("bwrap");
        cmd.args(self.build_args());
        cmd.arg("--");
        cmd.arg(program);
        cmd.args(program_args);
        cmd.current_dir(&self.work_dir);
        cmd
    }
}
```

```rust
// src/sandbox/mod.rs
pub mod bwrap;
```

**Step 4: Add `pub mod sandbox;` to `src/lib.rs`**

**Step 5: Run test to verify it passes**

```bash
cargo test sandbox
```

Expected: 2 tests PASS.

**Step 6: Commit**

```bash
git add src/sandbox/ tests/sandbox_test.rs
git commit -m "feat: add BwrapBuilder for filesystem-only sandboxing"
```

---

## Phase 5: Claude Sessions

### Task 6: ClaudeSession with --resume

**Files:**
- Modify: `src/session/claude.rs`
- Modify: `src/session/mod.rs`
- Create: `tests/session_test.rs`

**Step 1: Write failing test**

```rust
// tests/session_test.rs
use claude_chat::session::claude::{ClaudeSession, SessionExit};

#[tokio::test]
async fn session_returns_timeout_on_slow_command() {
    // Use a command that will definitely time out
    let session = ClaudeSession::new(
        "test-session".to_string(),
        "/tmp".to_string(),
        1, // 1 second timeout
    );
    let result = session.send_raw("sleep 10").await;
    assert!(matches!(result, Err(_)) || result.unwrap_or_default().contains("timeout"));
}

#[tokio::test]
async fn session_captures_stdout() {
    // Use echo as a stand-in for claude (does not require claude binary)
    let session = ClaudeSession::new_with_bin(
        "test-session".to_string(),
        "/tmp".to_string(),
        120,
        "echo", // use echo instead of claude
    );
    // send_raw calls: echo --resume test-session --dangerously-skip-permissions -p "hello"
    // echo just prints all args, which is fine for testing the subprocess plumbing
    let result = session.send_raw("hello").await.unwrap();
    assert!(!result.is_empty());
}

#[test]
fn session_id_from_room_alias() {
    let id = ClaudeSession::session_id_from_alias("nixos-agent:matrix.pin");
    assert_eq!(id, "nixos-agent");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test session
```

Expected: FAIL — module not found.

**Step 3: Implement ClaudeSession**

```rust
// src/session/claude.rs
use anyhow::Result;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use metrics::{counter, histogram, gauge};
use std::time::Instant;

pub enum SessionExit {
    Success(String),
    Timeout,
    Error(String),
}

pub struct ClaudeSession {
    pub session_id: String,
    pub work_dir: String,
    pub timeout_secs: u64,
    claude_bin: String,
}

impl ClaudeSession {
    pub fn new(session_id: String, work_dir: String, timeout_secs: u64) -> Self {
        Self::new_with_bin(
            session_id,
            work_dir,
            timeout_secs,
            std::env::var("CLAUDE_PATH").unwrap_or_else(|_| "claude".to_string()),
        )
    }

    pub fn new_with_bin(
        session_id: String,
        work_dir: String,
        timeout_secs: u64,
        bin: impl Into<String>,
    ) -> Self {
        Self { session_id, work_dir, timeout_secs, claude_bin: bin.into() }
    }

    /// Derive session ID from room alias (e.g. "nixos-agent:matrix.pin" → "nixos-agent")
    pub fn session_id_from_alias(alias: &str) -> String {
        alias.split(':').next().unwrap_or(alias).to_string()
    }

    /// Send a message and return Claude's response
    pub async fn send_raw(&self, text: &str) -> Result<String> {
        let start = Instant::now();
        let room = &self.session_id;

        counter!("claude_chat_session_started_total", "room" => room.clone()).increment(1);
        gauge!("claude_chat_session_active", "room" => room.clone()).set(1.0);

        let result = self.run_claude(text).await;
        let elapsed = start.elapsed().as_secs_f64();

        histogram!("claude_chat_session_duration_seconds", "room" => room.clone())
            .record(elapsed);
        gauge!("claude_chat_session_active", "room" => room.clone()).set(0.0);

        match &result {
            Ok(_) => counter!("claude_chat_session_completed_total",
                "room" => room.clone(), "exit" => "success").increment(1),
            Err(_) => counter!("claude_chat_session_completed_total",
                "room" => room.clone(), "exit" => "error").increment(1),
        }

        result
    }

    async fn run_claude(&self, text: &str) -> Result<String> {
        let mut cmd = Command::new(&self.claude_bin);
        cmd.args([
            "--resume", &self.session_id,
            "--dangerously-skip-permissions",
            "-p", text,
        ]);
        cmd.current_dir(&self.work_dir);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let child = cmd.spawn()?;

        let dur = Duration::from_secs(self.timeout_secs);
        match timeout(dur, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = if stdout.is_empty() { stderr } else { stdout };
                Ok(combined.trim().to_string())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("subprocess error: {e}")),
            Err(_) => {
                counter!("claude_chat_session_completed_total",
                    "room" => self.session_id.clone(), "exit" => "timeout").increment(1);
                Ok(format!("(timeout — no response after {}s)", self.timeout_secs))
            }
        }
    }
}
```

```rust
// src/session/mod.rs
pub mod claude;
pub mod manager;
```

**Step 4: Run tests to verify they pass**

```bash
cargo test session
```

Expected: PASS (timeout test may be slow — 1s).

**Step 5: Commit**

```bash
git add src/session/ tests/session_test.rs
git commit -m "feat: add ClaudeSession with --resume and timeout handling"
```

---

### Task 7: AgentState manager + crash recovery

**Files:**
- Modify: `src/session/manager.rs`

**Step 1: Write failing test**

```rust
// Add to tests/session_test.rs

#[tokio::test]
async fn agent_state_persists_last_event() {
    use claude_chat::session::manager::AgentState;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let store = dir.path().to_str().unwrap().to_string();

    let mut state = AgentState::load_or_create("test-agent", &store).await.unwrap();
    assert!(state.last_processed_event.is_none());

    state.last_processed_event = Some("$event_abc123".to_string());
    state.save().await.unwrap();

    // Reload
    let loaded = AgentState::load_or_create("test-agent", &store).await.unwrap();
    assert_eq!(loaded.last_processed_event.as_deref(), Some("$event_abc123"));
}

#[tokio::test]
async fn agent_state_history_appends() {
    use claude_chat::session::manager::{AgentState, HistoryEntry};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let store = dir.path().to_str().unwrap().to_string();
    let mut state = AgentState::load_or_create("test-agent", &store).await.unwrap();

    let entry = HistoryEntry {
        event_id: "$abc".to_string(),
        ts: chrono::Utc::now(),
        from: "@digger:matrix.pin".to_string(),
        text: "hello".to_string(),
        response_event: None,
        duration_ms: 1234,
        exit: "success".to_string(),
    };
    state.append_history(&entry).await.unwrap();

    // history.jsonl must exist and contain entry
    let history_path = dir.path().join("history.jsonl");
    assert!(history_path.exists());
    let content = std::fs::read_to_string(history_path).unwrap();
    assert!(content.contains("$abc"));
    assert!(content.contains("hello"));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test agent_state
```

Expected: FAIL.

**Step 3: Implement AgentState**

```rust
// src/session/manager.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AgentStatePersisted {
    pub last_processed_event: Option<String>,
    pub session_id: String,
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
            session_id: persisted.session_id.clone(),
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
        Ok(())
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test agent_state
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/session/manager.rs tests/session_test.rs
git commit -m "feat: add AgentState with crash-recovery via state.toml and history.jsonl"
```

---

## Phase 6: Matrix Client

### Task 8: Matrix login and sync loop

**Files:**
- Modify: `src/matrix/client.rs`
- Modify: `src/matrix/mod.rs`

**Step 1: Write failing test**

```rust
// tests/matrix_test.rs  (create file)
use claude_chat::matrix::client::derive_session_id;

#[test]
fn session_id_derived_from_room_alias() {
    assert_eq!(derive_session_id("#nixos-agent:matrix.pin"), "nixos-agent");
    assert_eq!(derive_session_id("nixos-agent:matrix.pin"), "nixos-agent");
    assert_eq!(derive_session_id("nixos-agent"), "nixos-agent");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test matrix
```

Expected: FAIL.

**Step 3: Implement client and login**

```rust
// src/matrix/client.rs
use anyhow::Result;
use matrix_sdk::{Client, config::SyncSettings};
use crate::config::Config;

pub fn derive_session_id(alias: &str) -> String {
    let stripped = alias.strip_prefix('#').unwrap_or(alias);
    stripped.split(':').next().unwrap_or(stripped).to_string()
}

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

pub async fn run_sync(client: Client) -> Result<()> {
    tracing::info!("starting Matrix sync loop");
    client.sync(SyncSettings::default()).await?;
    Ok(())
}
```

```rust
// src/matrix/mod.rs
pub mod client;
pub mod handler;
pub mod sender;
```

**Step 4: Run test to verify it passes**

```bash
cargo test matrix
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/matrix/ tests/matrix_test.rs
git commit -m "feat: add Matrix client login and sync loop"
```

---

### Task 9: Message handler with auth

**Files:**
- Modify: `src/matrix/handler.rs`
- Create: `tests/handler_test.rs`

**Step 1: Write failing test**

```rust
// tests/handler_test.rs
use claude_chat::matrix::handler::{MessageSource, classify_message, AuthResult, check_auth};

#[test]
fn classifies_control_room_command() {
    let result = classify_message("!list");
    assert!(matches!(result, MessageSource::ControlCommand(_)));
}

#[test]
fn classifies_agent_room_message() {
    let result = classify_message("update the flake");
    assert!(matches!(result, MessageSource::UserMessage(_)));
}

#[test]
fn classifies_inter_agent_message() {
    let result = classify_message("[from:nixos, depth:1] what version?");
    assert!(matches!(result, MessageSource::AgentMessage { .. }));

    if let MessageSource::AgentMessage { from, depth, text } = result {
        assert_eq!(from, "nixos");
        assert_eq!(depth, 1);
        assert_eq!(text, "what version?");
    }
}

#[test]
fn classifies_depth_exceeded() {
    let result = classify_message("[from:nixos, depth:3] recurse");
    assert!(matches!(result, MessageSource::DepthExceeded { .. }));
}

#[test]
fn auth_allows_permitted_user() {
    let allowed = vec!["@alice:host".to_string()];
    assert!(matches!(check_auth("@alice:host", &allowed), AuthResult::Allowed));
}

#[test]
fn auth_denies_unknown_user() {
    let allowed = vec!["@alice:host".to_string()];
    assert!(matches!(check_auth("@eve:host", &allowed), AuthResult::Denied));
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test handler
```

Expected: FAIL.

**Step 3: Implement handler types**

```rust
// src/matrix/handler.rs
use metrics::counter;

#[derive(Debug, PartialEq)]
pub enum MessageSource {
    ControlCommand(String),
    UserMessage(String),
    AgentMessage { from: String, depth: u8, text: String },
    DepthExceeded { from: String, depth: u8 },
}

#[derive(Debug)]
pub enum AuthResult {
    Allowed,
    Denied,
}

pub fn classify_message(text: &str) -> MessageSource {
    if text.starts_with("!") {
        return MessageSource::ControlCommand(text.to_string());
    }

    if let Some(rest) = text.strip_prefix("[from:") {
        if let Some(end) = rest.find(']') {
            let meta = &rest[..end];
            let msg_text = rest[end + 1..].trim().to_string();

            let from = meta.split(',').next().unwrap_or("unknown").trim().to_string();
            let depth: u8 = meta
                .split("depth:")
                .nth(1)
                .and_then(|d| d.trim().parse().ok())
                .unwrap_or(0);

            if depth >= 3 {
                return MessageSource::DepthExceeded { from, depth };
            }
            return MessageSource::AgentMessage { from, depth, text: msg_text };
        }
    }

    MessageSource::UserMessage(text.to_string())
}

pub fn check_auth(user_id: &str, allowed: &[String]) -> AuthResult {
    if allowed.iter().any(|u| u == user_id) {
        counter!("claude_chat_auth_checks_total",
            "user" => user_id.to_string(), "result" => "allowed").increment(1);
        AuthResult::Allowed
    } else {
        counter!("claude_chat_auth_checks_total",
            "user" => user_id.to_string(), "result" => "denied").increment(1);
        AuthResult::Denied
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test handler
```

Expected: 6 tests PASS.

**Step 5: Commit**

```bash
git add src/matrix/handler.rs tests/handler_test.rs
git commit -m "feat: add message handler with auth check and inter-agent prefix parsing"
```

---

### Task 10: Matrix room sender

**Files:**
- Modify: `src/matrix/sender.rs`

**Step 1: Write failing test**

```rust
// Add to tests/handler_test.rs
use claude_chat::matrix::sender::chunk_message;

#[test]
fn chunks_long_message() {
    let msg = "a".repeat(10000);
    let chunks = chunk_message(&msg, 4000);
    assert_eq!(chunks.len(), 3);
    for chunk in &chunks {
        assert!(chunk.len() <= 4000);
    }
}

#[test]
fn short_message_is_single_chunk() {
    let chunks = chunk_message("hello", 4000);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], "hello");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test chunk_message
```

Expected: FAIL.

**Step 3: Implement sender**

```rust
// src/matrix/sender.rs
use anyhow::Result;
use matrix_sdk::Room;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use metrics::counter;

pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    text.chars()
        .collect::<Vec<_>>()
        .chunks(max_len)
        .map(|c| c.iter().collect())
        .collect()
}

pub async fn send_text(room: &Room, text: &str) -> Result<()> {
    let room_id = room.room_id().to_string();
    for chunk in chunk_message(text, 4000) {
        let content = RoomMessageEventContent::text_plain(chunk);
        room.send(content).await?;
        counter!("claude_chat_matrix_messages_sent_total",
            "room" => room_id.clone(), "type" => "response").increment(1);
    }
    Ok(())
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test chunk_message
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/matrix/sender.rs tests/handler_test.rs
git commit -m "feat: add Matrix sender with message chunking"
```

---

## Phase 7: Sandbox + Session Integration

### Task 11: Sandboxed Claude invocation

**Files:**
- Modify: `src/session/claude.rs`

**Step 1: Write failing test**

```rust
// Add to tests/session_test.rs
use claude_chat::session::claude::ClaudeSession;
use claude_chat::sandbox::bwrap::BwrapBuilder;

#[tokio::test]
async fn sandboxed_session_uses_bwrap() {
    let bwrap = BwrapBuilder::new("/tmp/work", "/tmp/store");
    let session = ClaudeSession::new_sandboxed(
        "test-session".to_string(),
        "/tmp/work".to_string(),
        "/tmp/store".to_string(),
        5,
        "echo", // stand-in for claude
    );
    // Should not panic building the command
    let _ = session.build_command("hello");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test sandboxed_session
```

Expected: FAIL.

**Step 3: Add sandboxed constructor and `build_command`**

```rust
// Add to src/session/claude.rs

pub struct ClaudeSession {
    pub session_id: String,
    pub work_dir: String,
    pub store_dir: Option<String>, // None = no sandbox
    pub timeout_secs: u64,
    claude_bin: String,
}

impl ClaudeSession {
    // ... existing methods ...

    pub fn new_sandboxed(
        session_id: String,
        work_dir: String,
        store_dir: String,
        timeout_secs: u64,
        claude_bin: impl Into<String>,
    ) -> Self {
        Self {
            session_id,
            work_dir,
            store_dir: Some(store_dir),
            timeout_secs,
            claude_bin: claude_bin.into(),
        }
    }

    pub fn build_command(&self, text: &str) -> tokio::process::Command {
        if let Some(ref store) = self.store_dir {
            // Sandboxed: wrap with bwrap
            let bwrap = crate::sandbox::bwrap::BwrapBuilder::new(&self.work_dir, store);
            bwrap.wrap_command(&self.claude_bin, &[
                "--resume", &self.session_id,
                "--dangerously-skip-permissions",
                "-p", text,
            ])
        } else {
            // Unsandboxed (testing / control room)
            let mut cmd = tokio::process::Command::new(&self.claude_bin);
            cmd.args(["--resume", &self.session_id,
                      "--dangerously-skip-permissions", "-p", text]);
            cmd.current_dir(&self.work_dir);
            cmd
        }
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test sandboxed_session
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/session/claude.rs tests/session_test.rs
git commit -m "feat: integrate bwrap sandbox into ClaudeSession"
```

---

## Phase 8: Control Commands

### Task 12: Control room command handlers

**Files:**
- Create: `src/matrix/control.rs`

**Step 1: Write failing test**

```rust
// Add to tests/handler_test.rs
use claude_chat::matrix::control::parse_control_command;
use claude_chat::matrix::control::ControlCommand;

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
fn parses_grant_command() {
    let cmd = parse_control_command("!grant nixos github-token");
    assert!(matches!(cmd, Some(ControlCommand::Grant { ref agent, ref secret })
        if agent == "nixos" && secret == "github-token"));
}

#[test]
fn returns_none_for_unknown_command() {
    assert!(parse_control_command("!foobar").is_none());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test control
```

Expected: FAIL.

**Step 3: Implement control command parser**

```rust
// src/matrix/control.rs
use metrics::counter;

#[derive(Debug, PartialEq)]
pub enum ControlCommand {
    List,
    Status,
    Reset(String),
    Spawn(String),
    Kill(String),
    Audit(Option<String>),
    Grant { agent: String, secret: String },
    Revoke { agent: String, secret: String },
    Help,
}

pub fn parse_control_command(text: &str) -> Option<ControlCommand> {
    let text = text.strip_prefix('!').unwrap_or(text).trim();
    let mut parts = text.splitn(3, ' ');
    let cmd = parts.next()?;
    let arg1 = parts.next().map(str::trim).map(String::from);
    let arg2 = parts.next().map(str::trim).map(String::from);

    let result = match cmd {
        "list"   => Some(ControlCommand::List),
        "status" => Some(ControlCommand::Status),
        "help"   => Some(ControlCommand::Help),
        "reset"  => arg1.map(ControlCommand::Reset),
        "spawn"  => arg1.map(ControlCommand::Spawn),
        "kill"   => arg1.map(ControlCommand::Kill),
        "audit"  => Some(ControlCommand::Audit(arg1)),
        "grant"  => arg1.zip(arg2).map(|(agent, secret)| ControlCommand::Grant { agent, secret }),
        "revoke" => arg1.zip(arg2).map(|(agent, secret)| ControlCommand::Revoke { agent, secret }),
        _ => None,
    };

    if let Some(ref r) = result {
        counter!("claude_chat_control_commands_total",
            "command" => cmd.to_string()).increment(1);
    }
    result
}
```

**Step 4: Add `pub mod control;` to `src/matrix/mod.rs`**

**Step 5: Run test to verify it passes**

```bash
cargo test control
```

Expected: PASS.

**Step 6: Commit**

```bash
git add src/matrix/control.rs tests/handler_test.rs
git commit -m "feat: add control room command parser"
```

---

## Phase 9: Secrets MCP Server

### Task 13: Vault reader + policy enforcement

**Files:**
- Modify: `src/secrets/vault.rs`
- Create: `tests/secrets_test.rs`

**Step 1: Write failing test**

```rust
// tests/secrets_test.rs
use claude_chat::secrets::vault::{Vault, PolicyError};
use tempfile::TempDir;
use std::fs;

fn setup_vault(dir: &TempDir) -> Vault {
    // Create vault structure
    fs::create_dir_all(dir.path().join("vault")).unwrap();
    fs::write(dir.path().join("vault/github-token"), "ghp_test123\n").unwrap();
    fs::write(dir.path().join("vault/npm-token"), "npm_test456\n").unwrap();

    let policy = r#"
[agents.nixos]
allowed_secrets = ["github-token"]

[agents.claude-chat]
allowed_secrets = ["github-token", "npm-token"]
"#;
    fs::write(dir.path().join("policy.toml"), policy).unwrap();

    Vault::load(dir.path().to_str().unwrap()).unwrap()
}

#[test]
fn grants_access_to_allowed_secret() {
    let dir = TempDir::new().unwrap();
    let vault = setup_vault(&dir);
    let result = vault.read_secret("nixos", "github-token");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().trim(), "ghp_test123");
}

#[test]
fn denies_access_to_forbidden_secret() {
    let dir = TempDir::new().unwrap();
    let vault = setup_vault(&dir);
    let result = vault.read_secret("nixos", "npm-token");
    assert!(matches!(result, Err(PolicyError::Denied { .. })));
}

#[test]
fn denies_access_for_unknown_agent() {
    let dir = TempDir::new().unwrap();
    let vault = setup_vault(&dir);
    let result = vault.read_secret("unknown-agent", "github-token");
    assert!(matches!(result, Err(PolicyError::Denied { .. })));
}

#[test]
fn denies_nonexistent_secret() {
    let dir = TempDir::new().unwrap();
    let vault = setup_vault(&dir);
    let result = vault.read_secret("claude-chat", "nonexistent");
    assert!(result.is_err());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test secrets
```

Expected: FAIL.

**Step 3: Implement Vault**

```rust
// src/secrets/vault.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use metrics::counter;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("access denied: agent '{agent}' cannot read '{secret}'")]
    Denied { agent: String, secret: String },
    #[error("secret not found: '{0}'")]
    NotFound(String),
}

#[derive(Debug, Deserialize)]
struct Policy {
    agents: HashMap<String, AgentPolicy>,
}

#[derive(Debug, Deserialize)]
struct AgentPolicy {
    allowed_secrets: Vec<String>,
}

pub struct Vault {
    root: PathBuf,
    policy: Policy,
}

impl Vault {
    pub fn load(root: &str) -> Result<Self> {
        let root = PathBuf::from(root);
        let policy_path = root.join("policy.toml");
        let policy_str = std::fs::read_to_string(&policy_path)?;
        let policy: Policy = toml::from_str(&policy_str)?;
        Ok(Self { root, policy })
    }

    pub fn read_secret(&self, agent: &str, secret: &str) -> Result<String, PolicyError> {
        // Check policy
        let agent_policy = self.policy.agents.get(agent).ok_or_else(|| {
            counter!("claude_chat_mcp_secret_requests_total",
                "agent" => agent.to_string(), "secret" => secret.to_string(), "result" => "denied")
                .increment(1);
            PolicyError::Denied { agent: agent.to_string(), secret: secret.to_string() }
        })?;

        if !agent_policy.allowed_secrets.iter().any(|s| s == secret) {
            counter!("claude_chat_mcp_secret_requests_total",
                "agent" => agent.to_string(), "secret" => secret.to_string(), "result" => "denied")
                .increment(1);
            return Err(PolicyError::Denied { agent: agent.to_string(), secret: secret.to_string() });
        }

        // Read secret file
        let secret_path = self.root.join("vault").join(secret);
        let content = std::fs::read_to_string(&secret_path)
            .map_err(|_| PolicyError::NotFound(secret.to_string()))?;

        counter!("claude_chat_mcp_secret_requests_total",
            "agent" => agent.to_string(), "secret" => secret.to_string(), "result" => "granted")
            .increment(1);

        tracing::info!(
            service = "claude-chat",
            event = "secret_access",
            agent = agent,
            secret = secret,
            result = "granted"
        );

        Ok(content)
    }

    /// Read the public key for an agent from keys/<agent>.pub
    pub fn read_public_key(&self, agent: &str) -> Result<String> {
        let key_path = self.root.join("keys").join(format!("{}.pub", agent));
        Ok(std::fs::read_to_string(key_path)?.trim().to_string())
    }
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test secrets
```

Expected: 4 tests PASS.

**Step 5: Commit**

```bash
git add src/secrets/vault.rs tests/secrets_test.rs
git commit -m "feat: add secrets vault with policy enforcement and audit logging"
```

---

### Task 14: Age encryption for secrets delivery

**Files:**
- Modify: `src/secrets/vault.rs`
- Add tests to `tests/secrets_test.rs`

**Step 1: Write failing test**

```rust
// Add to tests/secrets_test.rs
use claude_chat::secrets::vault::encrypt_for_agent;

#[test]
fn encrypts_and_decrypts_secret() {
    use age::x25519;
    use std::io::Write;

    // Generate a test keypair
    let identity = x25519::Identity::generate();
    let pubkey = identity.to_public();
    let pubkey_str = pubkey.to_string();

    let plaintext = "ghp_test_token_value";
    let encrypted = encrypt_for_agent(plaintext, &pubkey_str).unwrap();

    // Encrypted blob must not contain plaintext
    let encrypted_str = String::from_utf8_lossy(&encrypted);
    assert!(!encrypted_str.contains(plaintext), "plaintext leaked into ciphertext");

    // Decrypt and verify round-trip
    let decrypted = claude_chat::secrets::vault::decrypt_with_identity(
        &encrypted,
        &identity.to_string(),
    ).unwrap();
    assert_eq!(decrypted.trim(), plaintext);
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test encrypts_and_decrypts
```

Expected: FAIL.

**Step 3: Implement encrypt/decrypt**

```rust
// Add to src/secrets/vault.rs

use age::{x25519, Encryptor, Decryptor};
use age::secrecy::ExposeSecret;
use std::io::{Read, Write};

pub fn encrypt_for_agent(plaintext: &str, pubkey_str: &str) -> Result<Vec<u8>> {
    let pubkey: x25519::Recipient = pubkey_str.parse()
        .map_err(|e| anyhow::anyhow!("invalid public key: {e}"))?;

    let encryptor = Encryptor::with_recipients(vec![Box::new(pubkey)])
        .map_err(|e| anyhow::anyhow!("encryptor error: {e}"))?;

    let mut output = Vec::new();
    let mut writer = encryptor.wrap_output(&mut output)?;
    writer.write_all(plaintext.as_bytes())?;
    writer.finish()?;
    Ok(output)
}

pub fn decrypt_with_identity(ciphertext: &[u8], identity_str: &str) -> Result<String> {
    let identity: x25519::Identity = identity_str.parse()
        .map_err(|e| anyhow::anyhow!("invalid identity: {e}"))?;

    let decryptor = Decryptor::new(ciphertext)?;
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    let mut output = String::new();
    reader.read_to_string(&mut output)?;
    Ok(output)
}
```

**Step 4: Run test to verify it passes**

```bash
cargo test encrypts_and_decrypts
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/secrets/vault.rs tests/secrets_test.rs
git commit -m "feat: add age X25519 encrypt/decrypt for secrets delivery"
```

---

### Task 15: MCP server (get_secret tool)

**Files:**
- Modify: `src/secrets/mcp_server.rs`
- Modify: `src/secrets/mod.rs`

**Step 1: Write failing test**

```rust
// Add to tests/secrets_test.rs
use claude_chat::secrets::mcp_server::SecretsVaultServer;

#[test]
fn mcp_server_builds_without_panic() {
    let dir = TempDir::new().unwrap();
    setup_vault(&dir);
    let server = SecretsVaultServer::new(dir.path().to_str().unwrap(), "nixos");
    assert!(server.is_ok());
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test mcp_server
```

Expected: FAIL.

**Step 3: Implement MCP server**

```rust
// src/secrets/mcp_server.rs
use anyhow::Result;
use super::vault::Vault;
use metrics::histogram;
use std::time::Instant;

/// MCP server exposing the get_secret tool to a Claude agent.
/// One instance per agent — knows which agent is calling.
pub struct SecretsVaultServer {
    vault: Vault,
    agent_name: String,
}

impl SecretsVaultServer {
    pub fn new(vault_root: &str, agent_name: &str) -> Result<Self> {
        let vault = Vault::load(vault_root)?;
        Ok(Self { vault, agent_name: agent_name.to_string() })
    }

    /// Handle a get_secret tool call.
    /// Returns age-encrypted blob as base64, or error message.
    pub fn handle_get_secret(&self, secret_name: &str) -> Result<String, String> {
        let start = Instant::now();

        // Read secret (enforces policy)
        let plaintext = self.vault.read_secret(&self.agent_name, secret_name)
            .map_err(|e| e.to_string())?;

        // Get agent's public key
        let pubkey = self.vault.read_public_key(&self.agent_name)
            .map_err(|e| format!("public key not found for {}: {e}", self.agent_name))?;

        // Encrypt
        let encrypted = super::vault::encrypt_for_agent(&plaintext, &pubkey)
            .map_err(|e| format!("encryption failed: {e}"))?;

        let elapsed = start.elapsed().as_secs_f64();
        histogram!("claude_chat_mcp_duration_seconds",
            "agent" => self.agent_name.clone(), "tool" => "get_secret")
            .record(elapsed);

        // Return as base64 so it's a clean string over stdio
        Ok(base64_encode(&encrypted))
    }

    /// System prompt fragment to inject into Claude for this agent
    pub fn system_prompt(&self, available_agents: &[String]) -> String {
        let agents_list = available_agents.join(", ");
        format!(r#"
You have the following tools available:

1. get_secret(name: string) -> string
   Fetches a secret by name. Returns the value age-encrypted with your public key.
   Decrypt with: echo "<value>" | base64 -d | age --decrypt -i ~/.agent-store/{agent}/agent.key
   Usage: <tool>get_secret("github-token")</tool>

2. send_to_agent(agent: string, message: string) -> string
   Sends a message to another agent and waits for its response.
   Available agents: {agents_list}
   Usage: <tool>send_to_agent("nixos", "rebuild the system")</tool>
"#, agent = self.agent_name)
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    // Simple base64 without dependency — use base64 crate in real impl
    // Add base64 = "0.22" to Cargo.toml
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data)
}
```

Add `base64 = "0.22"` to `Cargo.toml` dependencies.

```rust
// src/secrets/mod.rs
pub mod vault;
pub mod mcp_server;
```

**Step 4: Run test to verify it passes**

```bash
cargo test mcp_server
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/secrets/ tests/secrets_test.rs Cargo.toml
git commit -m "feat: add MCP secrets server with get_secret tool and system prompt injection"
```

---

## Phase 10: Inter-Agent Communication

### Task 16: Tool call parser + send_to_agent dispatch

**Files:**
- Modify: `src/agent/tool.rs`
- Modify: `src/agent/mod.rs`
- Create: `tests/agent_test.rs`

**Step 1: Write failing test**

```rust
// tests/agent_test.rs
use claude_chat::agent::tool::{parse_tool_calls, ToolCall};

#[test]
fn parses_send_to_agent_call() {
    let output = r#"I'll ask the other agent.
<tool>send_to_agent("nixos", "what is the current system generation?")</tool>
Waiting for response..."#;

    let calls = parse_tool_calls(output);
    assert_eq!(calls.len(), 1);
    if let ToolCall::SendToAgent { agent, message } = &calls[0] {
        assert_eq!(agent, "nixos");
        assert_eq!(message, "what is the current system generation?");
    } else {
        panic!("expected SendToAgent tool call");
    }
}

#[test]
fn parses_get_secret_call() {
    let output = r#"<tool>get_secret("github-token")</tool>"#;
    let calls = parse_tool_calls(output);
    assert_eq!(calls.len(), 1);
    assert!(matches!(&calls[0], ToolCall::GetSecret(name) if name == "github-token"));
}

#[test]
fn parses_multiple_tool_calls() {
    let output = r#"<tool>get_secret("npm-token")</tool>
Some text here.
<tool>send_to_agent("home", "sync packages")</tool>"#;
    let calls = parse_tool_calls(output);
    assert_eq!(calls.len(), 2);
}

#[test]
fn returns_empty_on_no_tool_calls() {
    let output = "Just a normal Claude response with no tools.";
    assert!(parse_tool_calls(output).is_empty());
}

#[test]
fn formats_inter_agent_message_with_depth() {
    use claude_chat::agent::tool::format_agent_message;
    let msg = format_agent_message("nixos", 0, "hello world");
    assert_eq!(msg, "[from:nixos, depth:0] hello world");
}
```

**Step 2: Run test to verify it fails**

```bash
cargo test agent
```

Expected: FAIL.

**Step 3: Implement tool parser**

```rust
// src/agent/tool.rs
use regex::Regex;

#[derive(Debug, PartialEq)]
pub enum ToolCall {
    SendToAgent { agent: String, message: String },
    GetSecret(String),
}

pub fn parse_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Match <tool>send_to_agent("agent", "message")</tool>
    let send_re = Regex::new(
        r#"<tool>send_to_agent\s*\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)</tool>"#
    ).unwrap();
    for cap in send_re.captures_iter(text) {
        calls.push(ToolCall::SendToAgent {
            agent: cap[1].to_string(),
            message: cap[2].to_string(),
        });
    }

    // Match <tool>get_secret("name")</tool>
    let secret_re = Regex::new(
        r#"<tool>get_secret\s*\(\s*"([^"]+)"\s*\)</tool>"#
    ).unwrap();
    for cap in secret_re.captures_iter(text) {
        calls.push(ToolCall::GetSecret(cap[1].to_string()));
    }

    calls
}

pub fn format_agent_message(from: &str, depth: u8, text: &str) -> String {
    format!("[from:{from}, depth:{depth}] {text}")
}
```

Add `regex = "1"` to `Cargo.toml` dependencies.

```rust
// src/agent/mod.rs
pub mod tool;
```

**Step 4: Run test to verify it passes**

```bash
cargo test agent
```

Expected: 5 tests PASS.

**Step 5: Commit**

```bash
git add src/agent/ tests/agent_test.rs Cargo.toml
git commit -m "feat: add tool call parser for send_to_agent and get_secret"
```

---

## Phase 11: Main Entry Point + Wiring

### Task 17: Wire everything together in main.rs

**Files:**
- Modify: `src/main.rs`

**Step 1: Write failing integration smoke test**

```rust
// Add to tests/config_test.rs

#[test]
fn config_default_path_is_not_empty() {
    let path = claude_chat::config::Config::default_path();
    assert!(!path.as_os_str().is_empty());
}
```

**Step 2: Implement main.rs**

```rust
// src/main.rs
mod config;
mod matrix;
mod session;
mod sandbox;
mod agent;
mod secrets;
mod observability;

use anyhow::Result;
use std::net::SocketAddr;
use crate::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Init logging (JSON to stdout → Vector → VictoriaLogs)
    observability::logging::init();

    // 2. Load config
    let config_path = std::env::var("CLAUDE_CHAT_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| Config::default_path());

    let config = Config::load(&config_path)?;
    tracing::info!(
        config = %config_path.display(),
        rooms = config.rooms.agents.len(),
        "configuration loaded"
    );

    // 3. Register + start metrics server
    observability::metrics::register_metrics();
    let metrics_addr: SocketAddr = format!("0.0.0.0:{}", config.observability.metrics_port)
        .parse()?;
    observability::metrics::start_metrics_server(metrics_addr).await?;

    // 4. Record startup metrics
    metrics::gauge!("claude_chat_rooms_configured")
        .set(config.rooms.agents.len() as f64);
    let start_time = std::time::Instant::now();

    // 5. Spawn uptime gauge updater
    tokio::spawn(async move {
        loop {
            metrics::gauge!("claude_chat_uptime_seconds")
                .set(start_time.elapsed().as_secs_f64());
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
        }
    });

    // 6. Build Matrix client
    let client = matrix::client::build_client(&config).await?;

    // 7. Register event handler
    let config_clone = config.clone();
    client.add_event_handler({
        let cfg = config_clone;
        move |event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
              room: matrix_sdk::Room| {
            let cfg = cfg.clone();
            async move {
                if let Err(e) = handle_message(event, room, &cfg).await {
                    tracing::error!(error = %e, "message handling error");
                }
            }
        }
    });

    tracing::info!("bot ready, starting sync");
    matrix::client::run_sync(client).await?;
    Ok(())
}

async fn handle_message(
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    room: matrix_sdk::Room,
    config: &Config,
) -> Result<()> {
    use matrix_sdk::ruma::events::room::message::MessageType;
    use matrix::handler::{classify_message, check_auth, AuthResult, MessageSource};

    let MessageType::Text(ref text_content) = event.content.msgtype else { return Ok(()); };
    let text = text_content.body.trim();
    let sender = event.sender.to_string();
    let room_id = room.room_id().to_string();

    tracing::info!(room = %room_id, sender = %sender, "message received");
    metrics::counter!("claude_chat_matrix_messages_received_total",
        "room" => room_id.clone()).increment(1);

    // Determine room type
    let is_control = room_id == config.rooms.control.room_id;

    if is_control {
        // Control room: owner only
        if !config.auth.default_allowed_users.contains(&sender) {
            tracing::warn!(sender = %sender, "unauthorized control room access");
            return Ok(());
        }
        handle_control_command(text, &room, config).await?;
        return Ok(());
    }

    // Agent room: find config
    let agent_entry = config.rooms.agents.iter()
        .find(|(_, a)| a.room_id == room_id);
    let Some((agent_name, agent_cfg)) = agent_entry else { return Ok(()); };

    // Auth check
    match check_auth(&sender, agent_cfg.effective_allowed_users(&config.auth.default_allowed_users)) {
        AuthResult::Denied => {
            tracing::warn!(sender = %sender, room = %room_id, "unauthorized agent room access");
            matrix::sender::send_text(&room, "Not authorized.").await?;
            return Ok(());
        }
        AuthResult::Allowed => {}
    }

    // Classify and dispatch
    match classify_message(text) {
        MessageSource::DepthExceeded { from, depth } => {
            tracing::warn!(from = %from, depth = depth, "inter-agent depth exceeded");
            metrics::counter!("claude_chat_agent_loop_rejected_total",
                "agent" => agent_name.clone()).increment(1);
        }
        MessageSource::UserMessage(_) | MessageSource::AgentMessage { .. } => {
            dispatch_to_claude(text, agent_name, agent_cfg, &room, config).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn dispatch_to_claude(
    text: &str,
    agent_name: &str,
    agent_cfg: &config::AgentConfig,
    room: &matrix_sdk::Room,
    config: &Config,
) -> Result<()> {
    let store_dir = &agent_cfg.store_dir;
    std::fs::create_dir_all(store_dir)?;

    let session = session::claude::ClaudeSession::new_sandboxed(
        config::AgentConfig::session_id(agent_name),
        agent_cfg.work_dir.clone(),
        store_dir.clone(),
        agent_cfg.timeout(),
        std::env::var("CLAUDE_PATH").unwrap_or_else(|_| "claude".to_string()),
    );

    let response = session.send_raw(text).await.unwrap_or_else(|e| {
        format!("Error: {e}")
    });

    matrix::sender::send_text(room, &response).await?;
    Ok(())
}

async fn handle_control_command(
    text: &str,
    room: &matrix_sdk::Room,
    config: &Config,
) -> Result<()> {
    use matrix::control::{parse_control_command, ControlCommand};

    let response = match parse_control_command(text) {
        Some(ControlCommand::List) => {
            let agents: Vec<String> = config.rooms.agents.keys()
                .map(|k| format!("• {k} (session: {k})"))
                .collect();
            format!("**Active agents:**\n{}", agents.join("\n"))
        }
        Some(ControlCommand::Help) => {
            "**Commands:** !list !status !reset <agent> !spawn <agent> !kill <agent> !audit [agent] !grant <agent> <secret> !revoke <agent> <secret>".to_string()
        }
        Some(ControlCommand::Status) => {
            format!("Rooms configured: {}", config.rooms.agents.len())
        }
        None => "Unknown command. Try !help".to_string(),
        _ => "Command received.".to_string(),
    };

    matrix::sender::send_text(room, &response).await?;
    Ok(())
}
```

**Step 3: Verify it compiles**

```bash
cargo check
```

Expected: no errors.

**Step 4: Run all tests**

```bash
cargo test
```

Expected: all tests PASS.

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire all modules together in main.rs with Matrix event loop"
```

---

## Phase 12: Grafana Dashboards

### Task 18: Claude Chat Overview dashboard

**Files:**
- Create: `~/git/nixos/modules/grafana/dashboards/claude-chat-overview.json`

**Step 1: Create dashboard JSON**

Use Grafana UI at `http://192.168.0.4:3000`:

1. Create new dashboard → "Claude Chat Overview"
2. Add panels:
   - **Messages/min**: `rate(claude_chat_matrix_messages_received_total[5m])` → timeseries by `room`
   - **Active Agents**: `sum(claude_chat_session_active)` → stat panel
   - **Response time p95**: `histogram_quantile(0.95, rate(claude_chat_session_duration_seconds_bucket[5m]))` → timeseries by `room`
   - **Error rate**: `rate(claude_chat_session_completed_total{exit!="success"}[5m])` → timeseries
   - **Auth Denied**: `sum(rate(claude_chat_auth_checks_total{result="denied"}[5m]))` → stat (red if > 0)
   - **Secret Access Log**: VictoriaLogs query `service:"claude-chat" AND event:"secret_access"` → logs panel
   - **Agent Store Disk**: `claude_chat_store_bytes` → bar gauge by `agent`
3. Save → Export → copy JSON
4. Write to `~/git/nixos/modules/grafana/dashboards/claude-chat-overview.json`

**Step 2: Commit to nixos repo**

```bash
cd ~/git/nixos
git add modules/grafana/dashboards/claude-chat-overview.json
git commit -m "feat(grafana): add Claude Chat overview dashboard"
```

---

### Task 19: Agent Detail dashboard

**Files:**
- Create: `~/git/nixos/modules/grafana/dashboards/claude-chat-agent.json`

**Step 1: Create dashboard with `$agent` variable**

Dashboard variable: `$agent` — values from `label_values(claude_chat_session_active, room)`.

Panels:
- **Response time p50/p95/p99**: `histogram_quantile(0.X, rate(session_duration_seconds_bucket{room="$agent"}[5m]))`
- **Commands by type**: `rate(claude_chat_command_executed_total{room="$agent"}[5m])` → bar chart grouped by `command`
- **Resume vs New**: `rate(claude_chat_session_resume_total{room="$agent",result=~"resumed|new"}[10m])` → pie chart
- **Logs**: VictoriaLogs `service:"claude-chat" AND room:"$agent"` → logs panel
- **SkyWalking traces link**: text panel with link to `http://192.168.0.4:8080/`

**Step 2: Commit**

```bash
cd ~/git/nixos
git add modules/grafana/dashboards/claude-chat-agent.json
git commit -m "feat(grafana): add Claude Chat agent detail dashboard"
```

---

### Task 20: Inter-Agent Traffic dashboard + Alert rules

**Files:**
- Create: `~/git/nixos/modules/grafana/dashboards/claude-chat-inter-agent.json`
- Create: `~/git/nixos/modules/grafana/alerts/claude-chat-alerts.json`

**Step 1: Inter-agent dashboard**

Panels:
- **Agent topology**: node graph panel — nodes = agents, edges = `claude_chat_agent_messages_sent_total{from, to}`
- **Messages heatmap**: `rate(agent_messages_sent_total[5m])` grouped by `from`+`to`
- **Roundtrip time**: `histogram_quantile(0.95, rate(agent_roundtrip_seconds_bucket[5m]))` by `from,to`

**Step 2: Alert rules — create in Grafana UI then export**

| Alert name | Query | Condition | For | Severity |
|------------|-------|-----------|-----|----------|
| `claude_chat_agent_down` | `claude_chat_session_active` | == 0 | 5m | warning |
| `claude_chat_high_error_rate` | `rate(session_completed{exit!="success"}[5m])` | > 0.5 | 2m | critical |
| `claude_chat_secret_denied` | `increase(mcp_secret_requests{result="denied"}[5m])` | > 0 | 0m | warning |
| `claude_chat_decrypt_failure` | `increase(mcp_secret_decrypt_errors_total[5m])` | > 0 | 0m | critical |
| `claude_chat_processing_lag` | `claude_chat_agent_processing_lag_seconds` | > 300 | 1m | warning |
| `claude_chat_stuck_agent` | `pending_messages > 0` AND `session_active == 0` | both true | 5m | critical |
| `claude_chat_matrix_sync_failing` | `rate(matrix_sync_errors_total[5m])` | > 1 | 2m | critical |
| `claude_chat_store_disk_full` | `claude_chat_store_bytes` | > 5e9 | 5m | warning |

**Step 3: Commit**

```bash
cd ~/git/nixos
git add modules/grafana/dashboards/ modules/grafana/alerts/
git commit -m "feat(grafana): add inter-agent dashboard and 8 alert rules for claude-chat"
```

---

## Phase 13: Nix Flake + Deployment

### Task 21: Update flake.nix for Rust binary

**Files:**
- Modify: `flake.nix`

**Step 1: Write failing test** (verify flake evaluates)

```bash
nix flake check
```

Expected: FAIL or warnings (Python package referenced in old flake).

**Step 2: Rewrite flake.nix**

```nix
{
  description = "Claude Chat Bot — Matrix/Claude multi-agent platform";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in {
        packages.claude-chat = rustPlatform.buildRustPackage {
          pname = "claude-chat";
          version = "0.5.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          buildInputs = with pkgs; [ openssl pkg-config ];
          nativeBuildInputs = with pkgs; [ pkg-config ];
        };

        packages.default = self.packages.${system}.claude-chat;

        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain pkgs.rust-analyzer pkgs.bubblewrap ];
          RUST_LOG = "claude_chat=debug";
        };
      }
    ) // {
      homeManagerModules.claude-chat = { config, lib, pkgs, ... }:
        let
          cfg = config.services.claude-chat;
          claudeChatPkg = self.packages.${pkgs.system}.claude-chat;
        in {
          options.services.claude-chat = {
            enable = lib.mkEnableOption "Claude Chat Matrix bot";
            configFile = lib.mkOption {
              type = lib.types.path;
              description = "Path to config.toml (chmod 600)";
            };
            claudePath = lib.mkOption {
              type = lib.types.str;
              default = "claude";
              description = "Full path to the claude binary";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.claude-chat = {
              Unit = {
                Description = "Claude Chat Matrix Bot";
                After = [ "network.target" ];
              };
              Service = {
                ExecStart = "${claudeChatPkg}/bin/claude-chat";
                Restart = "on-failure";
                RestartSec = "10s";
                Environment = [
                  "CLAUDE_CHAT_CONFIG=${cfg.configFile}"
                  "CLAUDE_PATH=${cfg.claudePath}"
                  "RUST_LOG=claude_chat=info"
                ];
              };
              Install.WantedBy = [ "default.target" ];
            };
          };
        };
    };
}
```

**Step 3: Generate Cargo.lock if missing**

```bash
cargo generate-lockfile
```

**Step 4: Check flake evaluates**

```bash
nix flake check
```

Expected: PASS (or only warnings about missing matrix Synapse).

**Step 5: Commit**

```bash
git add flake.nix Cargo.lock
git commit -m "feat: update Nix flake for Rust binary with buildRustPackage and HM module"
```

---

### Task 22: Update Home Manager config to use new module

**Files:**
- Modify: `~/git/home` — appropriate HM module

**Step 1: Add claude-chat input to home flake**

```bash
# In ~/git/home/flake.nix — add input:
claude-chat.url = "git+file:///home/digger/git/claude-chat";
# or when published:
# claude-chat.url = "github:abstract-bike/claude-chat";
```

**Step 2: Add config to HM module**

```nix
# In appropriate module (e.g., modules/services.nix):
imports = [ inputs.claude-chat.homeManagerModules.claude-chat ];

services.claude-chat = {
  enable = true;
  configFile = "/home/digger/.config/claude-chat/config.toml";
  claudePath = "/home/digger/.npm-global/bin/claude";
};
```

**Step 3: Create config.toml** (one-time setup)

```bash
mkdir -p ~/.config/claude-chat
# Write config.toml with real room IDs from your Matrix homeserver
chmod 600 ~/.config/claude-chat/config.toml
```

**Step 4: Apply**

```bash
home-manager switch --flake ~/git/home#heater
```

Expected: `claude-chat.service` started and active.

**Step 5: Verify service is running**

```bash
systemctl --user status claude-chat
journalctl --user -u claude-chat -f
```

**Step 6: Commit home config**

```bash
cd ~/git/home
git add .
git commit -m "feat: enable claude-chat Matrix bot via Home Manager"
```

---

## Phase 14: Cleanup

### Task 23: Delete legacy Python bot

**Files:**
- Delete: `bot/` directory
- Delete: `tests/test_*.py`
- Modify: `pyproject.toml` → remove (or repurpose)
- Modify: `pytest.ini` → remove

**Step 1: Verify Rust binary works end-to-end (smoke test)**

```bash
# Send a message in #claude-control:matrix.pin: "!list"
# Should see response in Matrix room
systemctl --user status claude-chat  # must be active
```

**Step 2: Delete Python files**

```bash
rm -rf bot/ tests/test_*.py pytest.ini pyproject.toml
```

**Step 3: Final cargo test**

```bash
cargo test
```

Expected: all tests PASS.

**Step 4: Final commit**

```bash
git add -A
git commit -m "chore: remove legacy Python Telegram bot (replaced by Rust Matrix bot)"
```

---

## Phase 15: Key Generation (One-Time Setup)

### Task 24: Generate agent keypairs

**Step 1: Install age via Home Manager** (add to `~/git/home/modules/packages.nix`)

```nix
pkgs.age
```

```bash
home-manager switch --flake ~/git/home#heater
```

**Step 2: Generate keypairs per agent**

```bash
for agent in nixos claude-chat home; do
  mkdir -p ~/.agent-store/$agent
  age-keygen -o ~/.agent-store/$agent/agent.key
  chmod 600 ~/.agent-store/$agent/agent.key
  # Extract public key
  mkdir -p ~/.agent-secrets/keys
  age-keygen -y ~/.agent-store/$agent/agent.key > ~/.agent-secrets/keys/$agent.pub
  echo "Generated keypair for $agent"
done
```

**Step 3: Create vault structure**

```bash
mkdir -p ~/.agent-secrets/vault
chmod 700 ~/.agent-secrets

# Add secrets (one file per secret, chmod 600)
echo "your_github_token" > ~/.agent-secrets/vault/github-token
chmod 600 ~/.agent-secrets/vault/github-token
```

**Step 4: Create policy.toml**

```bash
cat > ~/.agent-secrets/policy.toml << 'EOF'
[agents.nixos]
allowed_secrets = ["github-token"]

[agents.claude-chat]
allowed_secrets = ["github-token"]

[agents.home]
allowed_secrets = ["github-token"]
EOF
chmod 600 ~/.agent-secrets/policy.toml
```

**Step 5: Verify vault works**

```bash
# Test via bot: send "!list" in control room
# Then in an agent room, ask: get_secret("github-token")
# Should receive encrypted blob
```

---

## Summary

| Phase | Tasks | Deliverable |
|-------|-------|-------------|
| 1: Scaffolding | 1 | Rust project with all modules |
| 2: Observability | 2-3 | JSON logs + Prometheus /metrics |
| 3: Config | 4 | TOML config with per-agent auth |
| 4: Sandbox | 5 | BwrapBuilder (tested, no bwrap needed for tests) |
| 5: Sessions | 6-7 | ClaudeSession --resume + AgentState crash recovery |
| 6: Matrix | 8-10 | Login, handler, sender |
| 7: Integration | 11 | Sandboxed Claude invocation |
| 8: Control | 12 | Control room commands |
| 9: Secrets | 13-15 | Vault + age encryption + MCP server |
| 10: Inter-agent | 16 | Tool call parser + dispatch |
| 11: Wiring | 17 | main.rs end-to-end |
| 12: Grafana | 18-20 | 3 dashboards + 8 alerts |
| 13: Nix | 21-22 | flake.nix + HM deployment |
| 14: Cleanup | 23 | Delete Python code |
| 15: Keygen | 24 | Agent keypairs + vault |

**Total: 24 tasks, all TDD except Grafana dashboards and one-time setup.**
