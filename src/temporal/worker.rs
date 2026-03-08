use anyhow::Result;
use std::sync::Arc;
use temporalio_client::{Client as TemporalClient, ClientOptions, Connection, ConnectionOptions};
use temporalio_sdk::{Worker, WorkerOptions};
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions};
use matrix_sdk::Client as MatrixClient;

use crate::config::Config;
use crate::temporal::activities::ClaudeChatActivities;
use crate::temporal::workflow::AgentWorkflow;

pub async fn start_worker(
    config: Arc<Config>,
    matrix_client: Arc<MatrixClient>,
) -> Result<Worker> {
    let temporal_cfg = config.temporal.as_ref()
        .ok_or_else(|| anyhow::anyhow!("[temporal] config section required"))?;

    let runtime_options = RuntimeOptions::builder()
        .build()
        .map_err(|e| anyhow::anyhow!("RuntimeOptions build error: {e}"))?;
    let runtime = CoreRuntime::new_assume_tokio(runtime_options)?;

    let target: url::Url = temporal_cfg.endpoint.parse()?;

    let connection = Connection::connect(
        ConnectionOptions::new(target)
            .identity("claude-chat".to_string())
            .build(),
    ).await?;

    let temporal_client = TemporalClient::new(
        connection,
        ClientOptions::new(&temporal_cfg.namespace).build(),
    )?;

    let activities = ClaudeChatActivities {
        matrix_client,
        config: config.clone(),
    };

    let worker_options = WorkerOptions::new(&temporal_cfg.task_queue)
        .register_activities(activities)
        .register_workflow::<AgentWorkflow>()
        .max_cached_workflows(200)
        .build();

    Worker::new(&runtime, temporal_client, worker_options)
        .map_err(|e| anyhow::anyhow!("failed to create Temporal worker: {e}"))
}
