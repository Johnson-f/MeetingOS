use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{error, info, warn};

use crate::service::ServiceRegistry;

use super::{
    constants::{
        JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_MIGRATE_CHAT_VECTORS,
        JOB_PROCESS_RECALL_EVENT, JOB_SCHEDULE_MEETING_BOTS, JOB_SEND_SHARE_EMAILS,
        JOB_STORE_RECORDING_AUDIO, JOB_SYNC_GOOGLE_CALENDAR, JOB_TRANSCRIBE_RECORDING,
        JOB_VECTORIZE_CHAT_QA, JOB_VECTORIZE_TRANSCRIPT,
    },
    handlers::{
        fetch_recording_media_job, generate_note_job, migrate_chat_vectors_job,
        process_recall_event_job, schedule_meeting_bots_job, send_share_emails_job,
        store_recording_audio_job, sync_google_calendar_job, transcribe_recording_job,
        vectorize_chat_qa_job, vectorize_transcript_job,
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
        JOB_MIGRATE_CHAT_VECTORS => migrate_chat_vectors_job(services, &payload).await,
        JOB_SEND_SHARE_EMAILS => send_share_emails_job(services, &payload).await,
        _ => {
            info!(%job_type, "ignoring unknown job type");
            Ok(())
        }
    }
}

async fn run_periodic_scheduler(services: ServiceRegistry) {
    let mut bot_ticker = tokio::time::interval(TokioDuration::from_secs(120)); // 2 min
    let mut purge_ticker = tokio::time::interval(TokioDuration::from_secs(3600)); // 1 hour
    let mut watch_renewal_ticker = tokio::time::interval(TokioDuration::from_secs(21600)); // 6 hours

    // Skip the first immediate tick
    bot_ticker.tick().await;
    purge_ticker.tick().await;
    watch_renewal_ticker.tick().await;

    info!(
        "periodic scheduler started: bot scheduler every 2m, watch renewal every 6h, dead job purge every 1h"
    );

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
            _ = watch_renewal_ticker.tick() => {
                renew_expiring_watches(&services).await;
            }
            _ = purge_ticker.tick() => {
                match services.turso.purge_dead_jobs(7).await {
                    Ok(0) => {}
                    Ok(count) => info!(count, "purged dead jobs older than 7 days"),
                    Err(e) => warn!(error = %e, "failed to purge dead jobs"),
                }
            }
        }
    }
}

async fn renew_expiring_watches(services: &ServiceRegistry) {
    let google = match &services.google_calendar {
        Some(g) => g,
        None => return,
    };

    let public_url = match &services.config.public_app_url {
        Some(url) => url.clone(),
        None => {
            warn!("cannot renew watches: APP_PUBLIC_URL not configured");
            return;
        }
    };

    let webhook_url = format!("{}/api/v1/webhooks/google-calendar", public_url);

    let watches = match services.turso.get_expiring_watches(24).await {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "failed to fetch expiring watches");
            return;
        }
    };

    if watches.is_empty() {
        return;
    }

    info!(count = watches.len(), "renewing expiring calendar watches");

    for watch in &watches {
        // Refresh the access token first
        let access_token = if let Some(refresh_token) = &watch.refresh_token {
            match google.refresh_token(refresh_token).await {
                Ok(tokens) => {
                    let _ = services
                        .turso
                        .update_oauth_tokens(
                            &watch.oauth_connection_id,
                            &tokens.access_token,
                            tokens.refresh_token.as_deref(),
                        )
                        .await;
                    tokens.access_token
                }
                Err(e) => {
                    warn!(
                        connection_id = %watch.oauth_connection_id,
                        calendar = %watch.provider_calendar_id,
                        error = %e,
                        "failed to refresh token for watch renewal, marking auth_required"
                    );
                    let _ = services
                        .turso
                        .update_oauth_connection_status(&watch.oauth_connection_id, "auth_required")
                        .await;
                    continue;
                }
            }
        } else {
            warn!(
                connection_id = %watch.oauth_connection_id,
                "no refresh token for watch renewal"
            );
            continue;
        };

        // Stop the old watch (best-effort)
        let _ = google
            .stop_channel(&access_token, &watch.watch_channel_id, &watch.watch_resource_id)
            .await;

        // Register a new watch
        let new_channel_id = crate::service::turso::client::new_id();
        match google
            .watch_calendar(
                &access_token,
                &watch.provider_calendar_id,
                &new_channel_id,
                &webhook_url,
            )
            .await
        {
            Ok(new_watch) => {
                let _ = services
                    .turso
                    .update_calendar_watch(
                        &watch.oauth_connection_id,
                        &watch.provider_calendar_id,
                        &new_watch.channel_id,
                        &new_watch.resource_id,
                        &new_watch.expiration,
                    )
                    .await;
                info!(
                    calendar = %watch.provider_calendar_id,
                    old_channel = %watch.watch_channel_id,
                    new_channel = %new_watch.channel_id,
                    "renewed calendar watch"
                );
            }
            Err(e) => {
                warn!(
                    connection_id = %watch.oauth_connection_id,
                    calendar = %watch.provider_calendar_id,
                    error = %e,
                    "failed to renew calendar watch"
                );
            }
        }
    }
}

pub fn log_worker_shutdown(error: &tokio::task::JoinError) {
    error!(%error, "worker task exited unexpectedly");
}
