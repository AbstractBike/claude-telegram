// Matrix control room command parser
//
// Parses "!command [args]" messages from the control room into structured
// ControlCommand variants for dispatch by the handler layer.

use metrics::counter;

/// Commands available in the Matrix control room.
#[derive(Debug, PartialEq)]
pub enum ControlCommand {
    /// List all registered agents.
    List,
    /// Show platform status overview.
    Status,
    /// Reset a specific agent session.
    Reset(String),
    /// Spawn a new agent by name.
    Spawn(String),
    /// Kill (stop) a running agent.
    Kill(String),
    /// Show audit log, optionally filtered to one agent.
    Audit(Option<String>),
    /// Grant a secret to an agent.
    Grant { agent: String, secret: String },
    /// Revoke a secret from an agent.
    Revoke { agent: String, secret: String },
    /// Show available commands.
    Help,
}

/// Parse a control room message into a `ControlCommand`.
///
/// Returns `None` if the text does not start with `!` or is not a recognized command.
pub fn parse_control_command(text: &str) -> Option<ControlCommand> {
    let text = text.strip_prefix('!')?;
    let text = text.trim();
    let mut parts = text.splitn(3, ' ');
    let cmd = parts.next()?;
    let arg1 = parts.next().map(str::trim).map(String::from);
    let arg2 = parts.next().map(str::trim).map(String::from);

    let result = match cmd {
        "list" => Some(ControlCommand::List),
        "status" => Some(ControlCommand::Status),
        "help" => Some(ControlCommand::Help),
        "reset" => arg1.map(ControlCommand::Reset),
        "spawn" => arg1.map(ControlCommand::Spawn),
        "kill" => arg1.map(ControlCommand::Kill),
        "audit" => Some(ControlCommand::Audit(arg1)),
        "grant" => arg1
            .zip(arg2)
            .map(|(agent, secret)| ControlCommand::Grant { agent, secret }),
        "revoke" => arg1
            .zip(arg2)
            .map(|(agent, secret)| ControlCommand::Revoke { agent, secret }),
        _ => None,
    };

    if result.is_some() {
        counter!("claude_chat_control_commands_total", "command" => cmd.to_string()).increment(1);
    }

    result
}
