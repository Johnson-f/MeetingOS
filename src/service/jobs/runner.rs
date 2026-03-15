use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{error, info, warn};

use crate::service::ServiceRegistry;

use super::{
    constants::{
        JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_PROCESS_RECALL_EVENT,
        JOB_STORE_RECORDING_AUDIO, JOB_TRANSCRIBE_RECORDING,
    },
    handlers::{
        fetch_recording_media_job, generate_note_job, process_recall_event_job,
        store_recording_audio_job, transcribe_recording_job,
    },
};

pub async fn run_worker_loop(services: ServiceRegistry) -> Result<()> {
    let lease_owner = format!("worker-{}", uuid::Uuid::new_v4());
    let poll_interval = TokioDuration::from_millis(services.config.worker.poll_interval_ms);

    loop {
        match services
            .turso
            .lease_due_job(&lease_owner, services.config.worker.lease_seconds)
            .await?
        {
            Some(job) => {
                if let Err(error) = process_job(&services, &job.payload_json, &job.job_type).await {
                    warn!(job_id = %job.id, job_type = %job.job_type, %error, "job failed");

                    let next_run_after = if job.attempt_count + 1 >= job.max_attempts {
                        None
                    } else {
                        Some(
                            (Utc::now()
                                + Duration::seconds(
                                    2_i64.pow((job.attempt_count + 1).min(5) as u32),
                                ))
                            .to_rfc3339(),
                        )
                    };

                    services
                        .turso
                        .fail_job(
                            &job.id,
                            &error.to_string(),
                            next_run_after,
                            services.config.worker.max_attempts,
                        )
                        .await?;
                } else {
                    services.turso.complete_job(&job.id).await?;
                }
            }
            None => sleep(poll_interval).await,
        }
    }
}

async fn process_job(services: &ServiceRegistry, payload_json: &str, job_type: &str) -> Result<()> {
    let payload: Value = serde_json::from_str(payload_json).unwrap_or_else(|_| json!({}));

    match job_type {
        JOB_PROCESS_RECALL_EVENT => process_recall_event_job(services, &payload).await,
        JOB_FETCH_RECORDING_MEDIA => fetch_recording_media_job(services, &payload).await,
        JOB_STORE_RECORDING_AUDIO => store_recording_audio_job(services, &payload).await,
        JOB_TRANSCRIBE_RECORDING => transcribe_recording_job(services, &payload).await,
        JOB_GENERATE_NOTE => generate_note_job(services, &payload).await,
        _ => {
            info!(%job_type, "ignoring unknown job type");
            Ok(())
        }
    }
}

pub fn log_worker_shutdown(error: &tokio::task::JoinError) {
    error!(%error, "worker task exited unexpectedly");
}
