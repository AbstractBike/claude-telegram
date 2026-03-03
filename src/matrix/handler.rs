// Matrix event handler module

use metrics::counter;

/// Represents the classification of an incoming Matrix message.
#[derive(Debug, PartialEq)]
pub enum MessageSource {
    /// A control command starting with '!' (e.g. `!list`, `!status`).
    ControlCommand(String),
    /// A regular user message destined for the Claude session.
    UserMessage(String),
    /// A message forwarded from another agent with routing metadata.
    AgentMessage { from: String, depth: u8, text: String },
    /// An inter-agent message that exceeded the maximum recursion depth.
    DepthExceeded { from: String, depth: u8 },
}

/// Result of an authorization check.
#[derive(Debug)]
pub enum AuthResult {
    Allowed,
    Denied,
}

/// Classify an incoming message text.
///
/// `max_depth` is the configured limit for inter-agent call recursion.
/// Messages starting with `!` are control commands.
/// Messages with the `[from:<agent>, depth:<n>]` prefix are inter-agent messages.
/// Everything else is a regular user message.
pub fn classify_message(text: &str, max_depth: u8) -> MessageSource {
    if text.starts_with('!') || text.starts_with('/') {
        return MessageSource::ControlCommand(text.to_string());
    }

    if let Some(rest) = text.strip_prefix("[from:") {
        if let Some(end) = rest.find(']') {
            let meta = &rest[..end];
            let msg_text = rest[end + 1..].trim().to_string();

            let from = meta
                .split(',')
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_string();
            let depth: u8 = meta
                .split("depth:")
                .nth(1)
                .and_then(|d| d.trim().parse().ok())
                .unwrap_or(0);

            if depth >= max_depth {
                return MessageSource::DepthExceeded { from, depth };
            }
            return MessageSource::AgentMessage {
                from,
                depth,
                text: msg_text,
            };
        }
    }

    MessageSource::UserMessage(text.to_string())
}

/// Check whether `user_id` is in the `allowed` list.
///
/// Records a metric counter for each check with user and result labels.
pub fn check_auth(user_id: &str, allowed: &[String]) -> AuthResult {
    if allowed.iter().any(|u| u == user_id) {
        counter!("claude_chat_auth_checks_total",
            "user" => user_id.to_string(), "result" => "allowed")
        .increment(1);
        AuthResult::Allowed
    } else {
        counter!("claude_chat_auth_checks_total",
            "user" => user_id.to_string(), "result" => "denied")
        .increment(1);
        AuthResult::Denied
    }
}
