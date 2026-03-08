use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;
use temporalio_macros::{workflow, workflow_methods};
use temporalio_sdk::{
    SyncWorkflowContext, WorkflowContext, WorkflowContextView, WorkflowResult, WorkflowTermination,
    ActivityOptions,
};
use temporalio_common::protos::temporal::api::common::v1::RetryPolicy;
use temporalio_common::protos::coresdk::workflow_commands::ContinueAsNewWorkflowExecution;

use crate::temporal::activities::ClaudeChatActivities;

pub const TASK_QUEUE: &str = "claude-chat";
pub const WORKFLOW_TYPE: &str = "agent_conversation";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentWorkflowInput {
    pub agent_name: String,
    pub session_id: String,
    pub room_id: String,
    pub work_dir: String,
    pub store_dir: String,
    pub timeout_secs: u64,
    pub claude_bin: String,
    pub claude_home: Option<String>,
    pub vault_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IncomingMessage {
    pub text: String,
    pub from: String,
    pub event_id: String,
    pub depth: u8,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryRecord {
    pub event_id: String,
    pub from: String,
    pub text_preview: String,
    pub response_preview: String,
    pub duration_ms: u64,
    pub exit: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Processing { from: String },
    Resetting,
}

#[workflow]
pub struct AgentWorkflow {
    input: AgentWorkflowInput,
    queue: VecDeque<IncomingMessage>,
    history: Vec<HistoryRecord>,
    status: AgentStatus,
    reset_pending: bool,
}

#[workflow_methods]
impl AgentWorkflow {
    #[init]
    pub fn new(_ctx: &WorkflowContextView, input: AgentWorkflowInput) -> Self {
        Self {
            input,
            queue: VecDeque::new(),
            history: Vec::new(),
            status: AgentStatus::Idle,
            reset_pending: false,
        }
    }

    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<()> {
        loop {
            // Wait until there is a message in the queue
            ctx.wait_condition(|s: &AgentWorkflow| !s.queue.is_empty()).await;

            let msg = ctx.state_mut(|s| s.queue.pop_front()).unwrap();
            let input_snapshot = ctx.state(|s| s.input.clone());

            ctx.state_mut(|s| s.status = AgentStatus::Processing { from: msg.from.clone() });

            // Handle pending reset: clear session dir before running
            let needs_reset = ctx.state(|s| s.reset_pending);
            if needs_reset {
                let reset_input = crate::temporal::activities::ResetSessionInput {
                    store_dir: input_snapshot.store_dir.clone(),
                    session_id: input_snapshot.session_id.clone(),
                };
                let _ = ctx.start_activity(
                    ClaudeChatActivities::reset_session,
                    reset_input,
                    ActivityOptions {
                        start_to_close_timeout: Some(Duration::from_secs(10)),
                        ..Default::default()
                    },
                ).await;
                ctx.state_mut(|s| s.reset_pending = false);
            }

            // Run Claude
            let run_input = crate::temporal::activities::RunClaudeInput {
                agent_name: input_snapshot.agent_name.clone(),
                session_id: input_snapshot.session_id.clone(),
                work_dir: input_snapshot.work_dir.clone(),
                store_dir: input_snapshot.store_dir.clone(),
                timeout_secs: input_snapshot.timeout_secs,
                text: msg.text.clone(),
                event_id: msg.event_id.clone(),
                from: msg.from.clone(),
                claude_bin: input_snapshot.claude_bin.clone(),
                claude_home: input_snapshot.claude_home.clone(),
                vault_root: input_snapshot.vault_root.clone(),
            };

            let claude_result = ctx.start_activity(
                ClaudeChatActivities::run_claude,
                run_input,
                ActivityOptions {
                    start_to_close_timeout: Some(Duration::from_secs(input_snapshot.timeout_secs + 60)),
                    retry_policy: Some(RetryPolicy { maximum_attempts: 1, ..Default::default() }),
                    ..Default::default()
                },
            ).await;

            let output = match claude_result {
                Ok(o) => o,
                Err(e) => crate::temporal::activities::RunClaudeOutput {
                    response: format!("(activity error: {e})"),
                    duration_ms: 0,
                    exit: "error".to_string(),
                },
            };

            // Send Matrix response
            let send_input = crate::temporal::activities::SendMatrixInput {
                room_id: input_snapshot.room_id.clone(),
                text: output.response.clone(),
            };
            let _ = ctx.start_activity(
                ClaudeChatActivities::send_matrix_message,
                send_input,
                ActivityOptions {
                    start_to_close_timeout: Some(Duration::from_secs(30)),
                    retry_policy: Some(RetryPolicy { maximum_attempts: 3, ..Default::default() }),
                    ..Default::default()
                },
            ).await;

            // Update history
            ctx.state_mut(|s| {
                s.history.push(HistoryRecord {
                    event_id: msg.event_id.clone(),
                    from: msg.from.clone(),
                    text_preview: msg.text.chars().take(120).collect(),
                    response_preview: output.response.chars().take(120).collect(),
                    duration_ms: output.duration_ms,
                    exit: output.exit.clone(),
                });
                s.status = AgentStatus::Idle;
            });

            // Continue-as-new after 500 history entries to avoid bloating workflow history
            let should_continue = ctx.state(|s| s.history.len() >= 500);
            if should_continue {
                return Err(WorkflowTermination::continue_as_new(
                    ContinueAsNewWorkflowExecution {
                        workflow_type: WORKFLOW_TYPE.to_string(),
                        arguments: vec![],
                        task_queue: TASK_QUEUE.to_string(),
                        ..Default::default()
                    }
                ));
            }
        }
    }

    #[signal(name = "incoming_message")]
    pub fn incoming_message(&mut self, _ctx: &mut SyncWorkflowContext<Self>, msg: IncomingMessage) {
        self.queue.push_back(msg);
    }

    #[signal(name = "cancel")]
    pub fn cancel(&mut self, _ctx: &mut SyncWorkflowContext<Self>) {
        self.queue.clear();
        self.status = AgentStatus::Idle;
    }

    #[signal(name = "reset")]
    pub fn reset(&mut self, _ctx: &mut SyncWorkflowContext<Self>) {
        self.queue.clear();
        self.reset_pending = true;
        self.status = AgentStatus::Resetting;
    }

    #[query(name = "status")]
    pub fn status(&self, _ctx: &WorkflowContextView) -> AgentStatus {
        self.status.clone()
    }

    #[query(name = "history")]
    pub fn history(&self, _ctx: &WorkflowContextView, limit: u32) -> Vec<HistoryRecord> {
        let n = if limit == 0 { self.history.len() } else { limit as usize };
        self.history.iter().rev().take(n).cloned().collect()
    }
}

