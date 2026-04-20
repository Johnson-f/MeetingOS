use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{debug, error, info, warn};

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
    supervisor::supervised,
};

const WORKER_HEARTBEAT_SECS: i64 = 60;
const STUCK_JOB_AGE_SECS: i64 = 600; // 10 minutes
const STUCK_CHECK_INTERVAL: TokioDuration = TokioDuration::from_secs(300); // 5 min
const REAPER_INTERVAL: TokioDuration = TokioDuration::from_secs(60);

/// Entry point. Performs one-shot startup recovery, then spawns every
/// long-running background task under the supervisor.
///
/// After this returns, all tasks are owned by supervisors that restart them
/// on panic / error / unexpected clean exit. The returned future stays alive
/// by blocking on the consumer loop itself (supervised).
pub async fn run_worker_loop(services: ServiceRegistry) -> Result<()> {
    // Startup: recover any jobs that were leased but never completed (stale
    // leases from a previous process crash). Must run once, before the worker
    // starts leasing again, so we don't race the new lease TTLs.
    let recovered = services.turso.recover_stale_leases().await?;
    if recovered > 0 {
        info!(
            count = recovered,
            "recovered stale leased jobs from previous run"
        );
    }

    // Startup misconfig check: APP_PUBLIC_URL silently disables Google Calendar
    // push notifications. Log LOUDLY once at startup so it's visible.
    if services.google_calendar.is_some() && services.config.public_app_url.is_none() {
        error!(
            "APP_PUBLIC_URL is not configured — Google Calendar push notifications \
             are disabled (watch registration and renewal will be skipped). \
             Set APP_PUBLIC_URL to a public HTTPS URL."
        );
    }

    // ---- Supervised background tasks --------------------------------------
    // MUST be supervised — never spawn these directly.

    // Periodic scheduler: enqueues bot scheduling, runs watch renewal, purges
    // dead jobs. On panic, we lose the tickers until restart.
    let scheduler_services = services.clone();
    supervised("periodic_scheduler", move || {
        let services = scheduler_services.clone();
        async move { run_periodic_scheduler(services).await }
    });

    // Lease reaper: every 60s, requeues jobs whose lease has expired (or marks
    // dead if max_attempts exceeded). Without this, a worker that crashed
    // mid-job leaves the row parked forever.
    let reaper_services = services.clone();
    supervised("lease_reaper", move || {
        let services = reaper_services.clone();
        async move { run_lease_reaper(services).await }
    });

    // Stuck job detector: every 5 minutes, shouts if there are queued jobs
    // older than 10 minutes. Safety net against the same kind of incident
    // that prompted this file.
    let stuck_services = services.clone();
    supervised("stuck_job_detector", move || {
        let services = stuck_services.clone();
        async move { run_stuck_job_detector(services).await }
    });

    // Consumer loop. Runs directly on the caller's future so this function
    // stays alive. We wrap the inner consumer in its own supervisor so a panic
    // here doesn't kill the process — on restart the outer run_worker_loop
    // function is what the caller in main.rs is supervising.
    run_worker_consumer_loop(services).await
}

/// Consumer loop. Pulls leased jobs from the DB, dispatches to handlers,
/// emits a heartbeat every ~60s. Returns an error on DB failure (the outer
/// supervisor respawns it).
async fn run_worker_consumer_loop(services: ServiceRegistry) -> Result<()> {
    let lease_owner = format!("worker-{}", uuid::Uuid::new_v4());
    let poll_interval = TokioDuration::from_millis(services.config.worker.poll_interval_ms);
    let mut last_heartbeat = Utc::now();

    info!(lease_owner = %lease_owner, "worker consumer loop started");

    loop {
        // Heartbeat every WORKER_HEARTBEAT_SECS, emitted from inside this loop
        // so it can ONLY appear if the loop is iterating.
        let now = Utc::now();
        if (now - last_heartbeat).num_seconds() >= WORKER_HEARTBEAT_SECS {
            match services.turso.get_job_queue_stats().await {
                Ok(stats) => info!(
                    lease_owner = %lease_owner,
                    pending_count = stats.pending_count,
                    leased_count = stats.leased_count,
                    oldest_queued_age_seconds = stats.oldest_queued_age_seconds,
                    "worker heartbeat"
                ),
                Err(e) => warn!(error = %e, "worker heartbeat: failed to fetch queue stats"),
            }
            last_heartbeat = now;
        }

        match services
            .turso
            .lease_due_job(&lease_owner, services.config.worker.lease_seconds)
            .await?
        {
            Some(job) => {
                info!(
                    job_id = %job.id,
                    job_type = %job.job_type,
                    attempt = job.attempt_count,
                    "picked up job"
                );
                if let Err(error) = process_job(&services, &job.payload_json, &job.job_type).await
                {
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
                    info!(
                        job_id = %job.id,
                        job_type = %job.job_type,
                        "job completed successfully"
                    );
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

async fn run_periodic_scheduler(services: ServiceRegistry) -> Result<()> {
    let mut bot_ticker = tokio::time::interval(TokioDuration::from_secs(120)); // 2 min
    let mut purge_ticker = tokio::time::interval(TokioDuration::from_secs(3600)); // 1 hour
    let mut watch_renewal_ticker = tokio::time::interval(TokioDuration::from_secs(21600)); // 6 hours

    // Skip the first immediate tick
    bot_ticker.tick().await;
    purge_ticker.tick().await;
    watch_renewal_ticker.tick().await;

    info!(
        "periodic scheduler started: bot scheduler every 2m, watch renewal every 6h, \
         dead job purge every 1h"
    );

    loop {
        tokio::select! {
            _ = bot_ticker.tick() => {
                enqueue_periodic_bot_scheduler(&services).await;
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

/// Enqueue the periodic `schedule_meeting_bots` job, logging only on real
/// outcomes (not every tick). Dedupe-skipped is DEBUG, new row is INFO, error
/// is ERROR.
async fn enqueue_periodic_bot_scheduler(services: &ServiceRegistry) {
    match services
        .turso
        .enqueue_job(
            JOB_SCHEDULE_MEETING_BOTS,
            Some("periodic-schedule-bots"),
            &json!({}),
        )
        .await
    {
        Ok(true) => info!(
            job_type = JOB_SCHEDULE_MEETING_BOTS,
            dedupe_key = "periodic-schedule-bots",
            "enqueued periodic schedule_meeting_bots job"
        ),
        Ok(false) => debug!(
            job_type = JOB_SCHEDULE_MEETING_BOTS,
            dedupe_key = "periodic-schedule-bots",
            "skipped enqueue: existing active job with same dedupe_key"
        ),
        Err(e) => error!(
            job_type = JOB_SCHEDULE_MEETING_BOTS,
            error = %e,
            "failed to enqueue periodic schedule_meeting_bots job"
        ),
    }
}

/// Lease reaper: requeues jobs whose lease has expired. Max_attempts is
/// respected — expired leases whose next attempt would exceed the limit are
/// marked `dead` instead.
async fn run_lease_reaper(services: ServiceRegistry) -> Result<()> {
    info!(interval_seconds = REAPER_INTERVAL.as_secs(), "lease reaper started");
    let mut ticker = tokio::time::interval(REAPER_INTERVAL);
    ticker.tick().await; // skip immediate

    loop {
        ticker.tick().await;
        match services.turso.reap_expired_leases().await {
            Ok((0, 0)) => {}
            Ok((requeued, dead)) => info!(
                requeued_count = requeued,
                dead_count = dead,
                "reaped expired job leases"
            ),
            Err(e) => error!(error = %e, "lease reaper: failed to reap expired leases"),
        }
    }
}

/// Stuck-job detector: periodically checks whether any `queued` jobs have
/// been waiting longer than STUCK_JOB_AGE_SECS. Logs at ERROR if so.
/// Catches the exact incident class: queued rows that nothing is leasing.
async fn run_stuck_job_detector(services: ServiceRegistry) -> Result<()> {
    info!(
        interval_seconds = STUCK_CHECK_INTERVAL.as_secs(),
        threshold_seconds = STUCK_JOB_AGE_SECS,
        "stuck-job detector started"
    );
    let mut ticker = tokio::time::interval(STUCK_CHECK_INTERVAL);
    ticker.tick().await; // skip immediate

    loop {
        ticker.tick().await;
        match services.turso.find_stuck_queued_jobs(STUCK_JOB_AGE_SECS).await {
            Ok(None) => {}
            Ok(Some(report)) => error!(
                stuck_count = report.count,
                oldest_job_id = %report.oldest_job_id,
                oldest_job_type = %report.oldest_job_type,
                oldest_age_seconds = report.oldest_age_seconds,
                threshold_seconds = STUCK_JOB_AGE_SECS,
                "STUCK JOBS DETECTED: queued jobs not being picked up"
            ),
            Err(e) => warn!(error = %e, "stuck-job detector: query failed"),
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
            // Startup already logs this at ERROR. Per-tick noise is just debug.
            debug!("skipping watch renewal: APP_PUBLIC_URL not configured");
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
            .stop_channel(
                &access_token,
                &watch.watch_channel_id,
                &watch.watch_resource_id,
            )
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

