use anyhow::Result;
use metrics::{counter, gauge, histogram};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub struct ClaudeSession {
    pub session_id: String,
    pub work_dir: String,
    pub store_dir: Option<String>,
    pub timeout_secs: u64,
    claude_bin: String,
}

/// Resolve a binary path by following symlinks to get the real path.
/// This is needed for bwrap sandboxing, where symlinks outside the
/// sandbox (e.g. ~/.nix-profile/bin/claude) won't resolve.
fn resolve_bin(bin: impl Into<String>) -> String {
    let path = bin.into();
    std::fs::canonicalize(&path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(path)
}

impl ClaudeSession {
    pub fn new(session_id: String, work_dir: String, timeout_secs: u64) -> Self {
        let bin = std::env::var("CLAUDE_PATH").unwrap_or_else(|_| "claude".to_string());
        Self {
            session_id,
            work_dir,
            store_dir: None,
            timeout_secs,
            claude_bin: resolve_bin(bin),
        }
    }

    pub fn new_with_bin(
        session_id: String,
        work_dir: String,
        timeout_secs: u64,
        bin: impl Into<String>,
    ) -> Self {
        Self {
            session_id,
            work_dir,
            store_dir: None,
            timeout_secs,
            claude_bin: resolve_bin(bin),
        }
    }

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
            claude_bin: resolve_bin(claude_bin),
        }
    }

    pub fn session_id_from_alias(alias: &str) -> String {
        let stripped = alias.strip_prefix('#').unwrap_or(alias);
        stripped.split(':').next().unwrap_or(stripped).to_string()
    }

    pub fn build_command(&self, text: &str) -> Command {
        if let Some(ref store) = self.store_dir {
            let bwrap = crate::sandbox::bwrap::BwrapBuilder::new(&self.work_dir, store);
            bwrap.wrap_command(
                &self.claude_bin,
                &[
                    "--resume",
                    &self.session_id,
                    "--dangerously-skip-permissions",
                    "-p",
                    text,
                ],
            )
        } else {
            let mut cmd = Command::new(&self.claude_bin);
            cmd.args([
                "--resume",
                &self.session_id,
                "--dangerously-skip-permissions",
                "-p",
                text,
            ]);
            cmd.current_dir(&self.work_dir);
            cmd
        }
    }

    pub async fn send_raw(&self, text: &str) -> Result<String> {
        let start = Instant::now();
        let room = &self.session_id;

        counter!("claude_chat_session_started_total", "room" => room.clone()).increment(1);
        gauge!("claude_chat_session_active", "room" => room.clone()).set(1.0);

        let result = self.run_claude(text).await;
        let elapsed = start.elapsed().as_secs_f64();

        histogram!("claude_chat_session_duration_seconds", "room" => room.clone()).record(elapsed);
        gauge!("claude_chat_session_active", "room" => room.clone()).set(0.0);

        match &result {
            Ok(output) => {
                counter!(
                    "claude_chat_session_completed_total",
                    "room" => room.clone(),
                    "exit" => "success"
                )
                .increment(1);
                histogram!("claude_chat_session_output_bytes", "room" => room.clone())
                    .record(output.len() as f64);
            }
            Err(_) => {
                counter!(
                    "claude_chat_session_completed_total",
                    "room" => room.clone(),
                    "exit" => "error"
                )
                .increment(1);
            }
        }

        result
    }

    async fn run_claude(&self, text: &str) -> Result<String> {
        let mut cmd = self.build_command(text);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd.spawn()?;
        let dur = Duration::from_secs(self.timeout_secs);

        match timeout(dur, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                counter!(
                    "claude_chat_command_executed_total",
                    "room" => self.session_id.clone(),
                    "command" => "claude",
                    "exit_code" => output.status.code().unwrap_or(-1).to_string()
                )
                .increment(1);
                let combined = if stdout.trim().is_empty() {
                    stderr
                } else {
                    stdout
                };
                let result = combined.trim().to_string();
                if result.is_empty() {
                    Ok("(no response)".to_string())
                } else {
                    Ok(result)
                }
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("subprocess error: {e}")),
            Err(_) => {
                counter!(
                    "claude_chat_session_completed_total",
                    "room" => self.session_id.clone(),
                    "exit" => "timeout"
                )
                .increment(1);
                Ok(format!(
                    "(timeout -- no response after {}s)",
                    self.timeout_secs
                ))
            }
        }
    }
}
