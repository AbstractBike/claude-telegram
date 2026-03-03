# Claude Chat: Matrix Agent Platform — Design Document

**Date:** 2026-03-03
**Status:** Approved
**Scope:** Integrated migration from Telegram bot to Matrix-based multi-agent platform with sandboxing, persistent sessions, inter-agent communication, and full observability.

## Architecture

Single Rust binary (monolith), replacing the current Python Telegram bot.

```
claude-chat (Rust binary)
├── main.rs              — entry point, config, tokio runtime
├── config.rs            — TOML config loading
├── matrix/
│   ├── client.rs        — matrix-rust-sdk login, sync loop
│   ├── handler.rs       — dispatch by room type (control vs project)
│   └── sender.rs        — send messages to rooms
├── session/
│   ├── manager.rs       — HashMap<RoomId, AgentState>
│   └── claude.rs        — spawn claude --resume $id -p "text"
├── sandbox/
│   └── bwrap.rs         — wrap Command with bubblewrap args
├── agent/
│   └── tool.rs          — send_to_agent: write to another room via Matrix
├── secrets/
│   └── mcp_server.rs    — MCP secrets-vault server (crypto + policy + audit)
└── observability/
    ├── metrics.rs       — Prometheus /metrics endpoint
    ├── logging.rs       — JSON structured logs (tracing)
    └── tracing.rs       — SkyWalking spans
```

### Rust Dependencies

- `matrix-sdk` — async Matrix client
- `tokio` — async runtime + process management
- `serde` / `toml` — config parsing
- `tracing` + `tracing-subscriber` — structured JSON logging
- `skywalking` — distributed tracing
- `metrics` + `metrics-exporter-prometheus` — /metrics endpoint

## Matrix Rooms & Identity

### Room Structure

```
#claude-control:abstract.bike       — admin room (owner only)
#nixos-agent:abstract.bike          — project ~/git/nixos
#claude-chat-agent:abstract.bike    — project ~/git/claude-chat
#home-agent:abstract.bike           — project ~/git/home
```

Single bot user: `@claude-bot:abstract.bike`, joined to all rooms.

### Configuration

```toml
[matrix]
homeserver = "http://192.168.0.4:8008"
user = "@claude-bot:abstract.bike"
password_file = "/run/secrets/matrix-password"

[auth]
default_allowed_users = ["@digger:abstract.bike"]

[rooms.control]
room_id = "!abc123:abstract.bike"

[rooms.agents.nixos]
room_id = "!def456:abstract.bike"
work_dir = "/home/digger/git/nixos"
store_dir = "/home/digger/.agent-store/nixos"
timeout_secs = 300

[rooms.agents.claude-chat]
room_id = "!ghi789:abstract.bike"
work_dir = "/home/digger/git/claude-chat"
store_dir = "/home/digger/.agent-store/claude-chat"
allowed_users = ["@digger:abstract.bike", "@collaborator:abstract.bike"]

[rooms.agents.home]
room_id = "!jkl012:abstract.bike"
work_dir = "/home/digger/git/home"
store_dir = "/home/digger/.agent-store/home"
# Secret access controlled by MCP policy.toml, not here
```

### Authorization

- Room has `allowed_users` → use that.
- Room without → inherit `auth.default_allowed_users`.
- Control room → owner only, hardcoded.

## Sessions: Claude with `--resume`

### Session ID

Deterministic from room alias, survives bot restarts:

```
#nixos-agent:abstract.bike  →  session_id: "nixos-agent"
```

### Command Execution

```bash
claude --resume nixos-agent --dangerously-skip-permissions -p "user text"
```

Timeout: 120s default, configurable per room via `timeout_secs`.

### State

```rust
struct AgentState {
    config: AgentConfig,
    session_id: String,
    active_process: Option<Child>,
    last_processed_event: Option<OwnedEventId>,
}
```

`last_processed_event` persisted to `~/.agent-store/<name>/state.toml` for crash recovery.

### Message Processing History

Each agent maintains a local history at `~/.agent-store/<name>/history.jsonl`:

```json
{"event_id":"$abc","ts":"2026-03-03T14:22:01Z","from":"@digger:abstract.bike","text":"update the flake","response_event":"$def","duration_ms":34500,"exit":"success"}
```

This is also emitted as structured logs to VictoriaLogs — the file is local backup, Grafana is the canonical source.

### Control Commands (`#claude-control`)

| Command | Action |
|---------|--------|
| `!list` | List agents and their session IDs |
| `!status` | Show running processes per room |
| `!reset <agent>` | Delete session ID, next message starts fresh |
| `!spawn <agent>` | Send initial message to wake agent |
| `!kill <agent>` | Kill running Claude process |
| `!audit` | Show recent secret access logs |
| `!audit <agent>` | Filter by agent |
| `!grant <agent> <secret>` | Add secret permission at runtime |
| `!revoke <agent> <secret>` | Revoke secret permission |

## Bubblewrap Sandboxing

### Principle

Each agent sees only its workdir (r/w), its persistent store (r/w), and minimal system (r/o). Nothing else.

### Filesystem Visibility (example: nixos agent)

```
/nix              → ro-bind (Nix runtime, binaries)
/usr              → ro-bind (basic coreutils)
/etc/resolv.conf  → ro-bind (DNS)
/proc, /dev       → minimal

~/git/nixos/                    → bind r/w (workdir)
~/.agent-store/nixos/           → bind r/w (persistent installs)
/tmp/agent-nixos/               → tmpfs (ephemeral per session)
```

### What is NOT visible

- Other repos (`~/git/home/`, `~/git/claude-chat/`)
- Secrets (`~/.config/`, `~/.ssh/`, `~/.gnupg/`)
- Home directory — nothing outside explicitly mounted paths

### Persistent Install Store

Agents CAN install packages (`pip install`, `cargo install`, `npm install -g`). Everything lands in their `store_dir` or `/tmp`. They cannot see or modify anything on the rest of the machine.

### Resulting Command

```bash
bwrap \
  --ro-bind /nix /nix \
  --ro-bind /usr /usr \
  --ro-bind /etc/resolv.conf /etc/resolv.conf \
  --proc /proc \
  --dev /dev \
  --tmpfs /tmp \
  --bind /home/digger/git/nixos /home/digger/git/nixos \
  --bind /home/digger/.agent-store/nixos /home/digger/.agent-store/nixos \
  --unshare-all \
  --share-net \
  --die-with-parent \
  -- claude --resume nixos-agent --dangerously-skip-permissions -p "text"
# No --setenv: secrets are fetched on-demand via MCP, encrypted with agent's public key
```

`--share-net`: network allowed for now (monitoring added later).
`--die-with-parent`: sandbox dies if bot dies.

## Secrets: MCP Server + Agent Keypairs

### Principle

Zero plaintext secrets in env vars or bot memory. Each agent has its own keypair. Secrets travel encrypted end-to-end via an MCP server.

### Architecture

```
MCP Server (secrets-vault)
├── vault/                    — secrets encrypted at rest (age)
│   ├── github-token
│   ├── npm-token
│   ├── openai-key
│   └── grafana-api-key
├── policy.toml               — which agent can read what
├── keys/
│   ├── nixos.pub             — public key of nixos agent
│   ├── claude-chat.pub
│   └── home.pub
└── tools:
    └── get_secret(name) → encrypted blob

Agent (inside sandbox)
├── ~/.agent-store/<name>/
│   └── agent.key             — private key (only this agent can read it)
```

### Flow

```
Claude (nixos) needs github-token
  → Claude invokes MCP tool: get_secret("github-token")
  → MCP Server:
      → identifies calling agent → "nixos"
      → checks policy.toml → nixos can read github-token? YES
      → reads secret from vault
      → encrypts with nixos.pub (age)
      → logs: secret_access{agent="nixos", secret="github-token", result="granted"}
      → returns: encrypted blob
  → Claude (nixos):
      → decrypts with agent.key
      → uses the token
  → If not authorized:
      → MCP returns error: "access denied"
      → logs: secret_access{agent="nixos", secret="openai-key", result="denied"}
```

### Advantages Over Env Var Injection

| Aspect | Env vars | MCP + crypto |
|--------|----------|--------------|
| Bot sees the secret | Yes | Never |
| Secret in `/proc/<pid>/environ` | Yes, plaintext | No |
| Secret in bot memory | Yes | Never |
| Agent requests only what it needs | No, all injected at startup | Yes, on-demand |
| Audit granularity | Only at spawn time | Every individual access |

### Policy

```toml
[agents.nixos]
allowed_secrets = ["github-token"]

[agents.claude-chat]
allowed_secrets = ["github-token", "npm-token"]

[agents.home]
allowed_secrets = ["github-token"]
```

### Key Generation (one-time setup)

```bash
# Per agent:
age-keygen -o ~/.agent-store/nixos/agent.key
age-keygen -y ~/.agent-store/nixos/agent.key > ~/.agent-secrets/keys/nixos.pub
```

Private key lives inside the agent's sandbox. Public key lives in the MCP server. The bot never touches either.

### MCP Server Config

```toml
[vault]
path = "/home/digger/.agent-secrets/vault"
policy = "/home/digger/.agent-secrets/policy.toml"
keys_dir = "/home/digger/.agent-secrets/keys"

[crypto]
algorithm = "age"
```

Audit log lives in VictoriaLogs, queryable in Grafana: `service:"claude-chat" AND event:"secret_access"`.

## Inter-Agent Communication

### Layer 1: Tool Use (Claude → Claude)

Claude receives a tool via system prompt:

```
send_to_agent(agent: string, message: string) -> string

Sends a message to another agent and waits for its response.
Available agents: nixos, claude-chat, home
```

Flow:

```
Claude (nixos) invokes: send_to_agent("claude-chat", "what is your version?")
  → Bot parses tool call from Claude stdout
  → Bot writes to #claude-chat-agent: "[from:nixos, depth:0] what is your version?"
  → Bot waits for claude-chat agent response
  → Claude (claude-chat) responds: "0.5.0"
  → Bot returns "0.5.0" as tool result to Claude (nixos)
```

### Layer 2: Direct Message (anyone → agent)

Any message in an agent room is processed. No tool call needed — humans, scripts, webhooks can all write directly.

### Message Format

```
[from:nixos, depth:1] message here
```

Prefix identifies source agent and call depth. No prefix = human or external source.

### Queue = Matrix Room Timeline

The room timeline IS the queue. Bot only tracks which messages have been processed.

```
Bot starts / reconnects
  → reads timeline from last_processed_event
  → processes pending messages in FIFO order
  → updates last_processed_event after each

New message arrives
  → agent free? process immediately
  → agent busy? do nothing, message is already in Matrix
    → when current finishes, read next from timeline
```

Crash → restart → bot reads `state.toml` → resumes from last processed message. Zero loss.

### Loop Prevention

Max depth header prevents infinite recursion:

```toml
[inter_agent]
timeout_secs = 180
max_depth = 3
```

```
depth:3 → REJECTED: max depth exceeded
```

## Observability

### Principle: If the bot does it, there is a metric for it.

### Metrics (Prometheus `/metrics` endpoint)

```
# Matrix
claude_chat_matrix_messages_received_total{room, user, type}
claude_chat_matrix_messages_sent_total{room, type}
claude_chat_matrix_sync_duration_seconds
claude_chat_matrix_sync_errors_total{error}
claude_chat_matrix_api_requests_total{method, endpoint, status}
claude_chat_matrix_api_duration_seconds{method, endpoint}

# Auth
claude_chat_auth_checks_total{room, user, result}

# Sessions
claude_chat_session_started_total{room}
claude_chat_session_completed_total{room, exit}
claude_chat_session_duration_seconds{room}
claude_chat_session_active{room}
claude_chat_session_output_bytes{room}
claude_chat_session_resume_total{room, result}

# Commands (every subprocess the bot executes)
claude_chat_command_executed_total{room, command, exit_code}
claude_chat_command_duration_seconds{room, command}
claude_chat_command_stdout_bytes{room, command}

# Sandbox
claude_chat_bwrap_spawns_total{room}
claude_chat_bwrap_failures_total{room, reason}
claude_chat_store_bytes{agent}

# Secrets (MCP)
claude_chat_mcp_secret_requests_total{agent, secret, result}
claude_chat_mcp_secret_decrypt_errors_total{agent}
claude_chat_mcp_requests_total{agent, tool, status}
claude_chat_mcp_duration_seconds{agent, tool}

# Inter-agent
claude_chat_agent_messages_sent_total{from, to}
claude_chat_agent_messages_received_total{from, to}
claude_chat_agent_roundtrip_seconds{from, to}
claude_chat_agent_tool_calls_total{from, to, result}
claude_chat_agent_queue_rejected_total{agent}
claude_chat_agent_loop_rejected_total{from, to}

# Queue (timeline-based)
claude_chat_agent_pending_messages{agent}
claude_chat_agent_processing_lag_seconds{agent}
claude_chat_agent_messages_processed_total{agent, from_type, exit}

# HTTP
claude_chat_http_requests_total{method, path, status}
claude_chat_http_duration_seconds{method, path}

# Control
claude_chat_control_commands_total{command, user}

# System
claude_chat_uptime_seconds
claude_chat_rooms_configured
claude_chat_rooms_active
```

### Logs: JSON → Vector → VictoriaLogs

Every log line carries: `service`, `host`, `level`, `room`, `user`, `trace_id`, `sw8` (SkyWalking context).

### Traces: SkyWalking

```
[message_received]      ← entry span
  ├── [auth_check]
  ├── [secret_inject]
  ├── [bwrap_spawn]
  ├── [claude_execute]  ← exit span
  ├── [matrix_reply]    ← exit span
  └── [agent_forward]   ← exit span (inter-agent)
```

SkyWalking endpoint: `http://192.168.0.4:11800` (gRPC collector).

### Dashboards

**Dashboard 1: Claude Chat Overview** — messages/min, active agents, command execution rate + duration p95, HTTP requests + status codes, auth denied count, secret access log table, agent store disk usage.

**Dashboard 2: Agent Detail** (variable: `$agent`) — response time p50/p95/p99, timeouts/errors, commands by type, resume vs new sessions, SkyWalking traces, filtered logs.

**Dashboard 3: Inter-Agent Traffic** — agent topology (node graph panel), messages between agents (heatmap), roundtrip time by pair.

### Alerts

| Alert | Condition | Severity |
|-------|-----------|----------|
| Agent down | `session_active == 0` for 5min | warning |
| High error rate | `rate(session_completed{exit!="success"}[5m]) > 0.5` | critical |
| Secret denied | Any `mcp_secret_requests{result="denied"}` | warning |
| Decrypt failure | `mcp_secret_decrypt_errors_total` > 0 | critical |
| Unusual secret access | Agent requests secret it never requested before | warning |
| Store disk full | `store_bytes > 5GB` | warning |
| Timeout spike | `rate(session_completed{exit="timeout"}[10m]) > 3` | critical |
| HTTP errors | `rate(http_requests{status=~"5.."}[5m]) > 0` | warning |
| Command crash | `command_executed{exit_code!="0"}` spike | warning |
| Matrix sync failing | `rate(sync_errors_total[5m]) > 1` | critical |
| Processing lag | `processing_lag_seconds > 300` | warning |
| Stuck agent | `pending_messages > 0` AND `session_active == 0` for 5min | critical |

## Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Architecture | Integrated plan (Matrix → sessions → sandbox → inter-agent) | Features are interdependent |
| Language | Rust | `matrix-rust-sdk` is the official SDK, static binary, Nix-friendly |
| Rooms | Hybrid: control + one per project | Natural mapping to repos, admin needs a control plane |
| Sandbox | Bubblewrap filesystem-only | Network monitoring added later |
| Sessions | `claude --resume` per message | Robust to crashes, simpler than PTY |
| Inter-agent | Tool use + direct Matrix messages | Matrix is the universal bus, tool is syntactic sugar |
| Queue | Matrix room timeline | Already persistent, zero state in memory |
| Secrets | MCP server + age keypairs per agent | Zero plaintext in bot/env vars, on-demand encrypted delivery, per-access audit |
| Observability | VictoriaMetrics + VictoriaLogs + SkyWalking + Grafana | Full MDD compliance |
| Message broker | None (Matrix IS the bus) | Adding Kafka over Matrix is a bus on a bus |
