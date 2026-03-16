use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{error, info, warn};

use crate::service::ServiceRegistry;

use super::{
    constants::{
        JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_PROCESS_RECALL_EVENT,
        JOB_SCHEDULE_MEETING_BOTS, JOB_STORE_RECORDING_AUDIO, JOB_SYNC_GOOGLE_CALENDAR,
        JOB_TRANSCRIBE_RECORDING, JOB_VECTORIZE_CHAT_QA, JOB_VECTORIZE_TRANSCRIPT,
    },
    handlers::{
        fetch_recording_media_job, generate_note_job, process_recall_event_job,
        schedule_meeting_bots_job, store_recording_audio_job, sync_google_calendar_job,
        transcribe_recording_job, vectorize_chat_qa_job, vectorize_transcript_job,
    },
};

pub async fn run_worker_loop(services: ServiceRegistry) -> Result<()> {
    // Recover any jobs that were leased but never completed (stale leases from a crash)
    let recovered = services.turso.recover_stale_leases().await?;
    if recovered > 0 {
        info!(
            count = recovered,
            "recovered stale leased jobs from previous run"
        );
    }

    let lease_owner = format!("worker-{}", uuid::Uuid::new_v4());
    let poll_interval = TokioDuration::from_millis(services.config.worker.poll_interval_ms);

    // Spawn periodic job scheduler
    let scheduler_services = services.clone();
    tokio::spawn(async move {
        run_periodic_scheduler(scheduler_services).await;
    });

    loop {
        match services
            .turso
            .lease_due_job(&lease_owner, services.config.worker.lease_seconds)
            .await?
        {
            Some(job) => {
                info!(job_id = %job.id, job_type = %job.job_type, attempt = job.attempt_count, "picked up job");
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
                    info!(job_id = %job.id, job_type = %job.job_type, "job completed successfully");
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
        JOB_VECTORIZE_TRANSCRIPT => vectorize_transcript_job(services, &payload).await,
        JOB_VECTORIZE_CHAT_QA => vectorize_chat_qa_job(services, &payload).await,
        JOB_SYNC_GOOGLE_CALENDAR => sync_google_calendar_job(services, &payload).await,
        JOB_SCHEDULE_MEETING_BOTS => schedule_meeting_bots_job(services, &payload).await,
        _ => {
            info!(%job_type, "ignoring unknown job type");
            Ok(())
        }
    }
}

async fn run_periodic_scheduler(services: ServiceRegistry) {
    let mut bot_ticker = tokio::time::interval(TokioDuration::from_secs(120)); // 2 min
    let mut sync_ticker = tokio::time::interval(TokioDuration::from_secs(900)); // 15 min

    // Skip the first immediate tick
    bot_ticker.tick().await;
    sync_ticker.tick().await;

    info!("periodic scheduler started: bot scheduler every 2m, calendar sync every 15m");

    loop {
        tokio::select! {
            _ = bot_ticker.tick() => {
                info!("enqueuing periodic schedule_meeting_bots job");
                let _ = services.turso.enqueue_job(
                    JOB_SCHEDULE_MEETING_BOTS,
                    Some("periodic-schedule-bots"),
                    &json!({}),
                ).await;
            }
            _ = sync_ticker.tick() => {
                info!("enqueuing periodic calendar sync jobs");
                // Enqueue a sync job for each active Google OAuth connection
                match services.turso.get_all_active_oauth_connections("google").await {
                    Ok(connections) => {
                        info!(count = connections.len(), "found active Google OAuth connections");
                        for conn in connections {
                            let _ = services.turso.enqueue_job(
                                JOB_SYNC_GOOGLE_CALENDAR,
                                Some(&format!("periodic-sync-{}", conn.id)),
                                &json!({
                                    "oauth_connection_id": conn.id,
                                    "user_id": conn.user_id,
                                    "workspace_id": conn.workspace_id,
                                }),
                            ).await;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to fetch OAuth connections for periodic sync");
                    }
                }
            }
        }
    }
}

pub fn log_worker_shutdown(error: &tokio::task::JoinError) {
    error!(%error, "worker task exited unexpectedly");
}
