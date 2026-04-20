use std::future::Future;
use std::time::Duration;

use rand::Rng;
use tokio::time::Instant;
use tracing::{error, info, warn};

/// First retry delay. Subsequent delays double until `MAX_BACKOFF`.
const BASE_BACKOFF: Duration = Duration::from_secs(3);
/// Upper bound on the retry delay — a persistently broken task will not spam
/// faster than this.
const MAX_BACKOFF: Duration = Duration::from_secs(300); // 5 minutes
/// If a task ran at least this long before dying, reset the backoff — healthy
/// tasks that occasionally hiccup get fast recovery.
const HEALTHY_RUN_THRESHOLD: Duration = Duration::from_secs(60);
/// After this many consecutive failures, downgrade restart log to `warn!` so a
/// persistent issue shows up as one ERROR and then becomes filterable noise.
const LOUD_FAILURE_LIMIT: u32 = 5;
/// Upper bound on jitter added to each backoff delay.
const MAX_JITTER_MS: u64 = 2_000;

/// Supervise a long-running background task.
///
/// The supervisor awaits the inner `JoinHandle` and restarts the task on:
///   - panic
///   - `Err` return
///   - unexpected clean exit (a task that's supposed to run forever returning `Ok(())`)
///
/// Between restarts it sleeps with exponential backoff: starts at 3s, doubles
/// after each failure, caps at 5 minutes. Jitter (0-2s) is added each time.
/// If a task ran cleanly for at least `HEALTHY_RUN_THRESHOLD` before dying,
/// the backoff resets to the base — so a healthy task that briefly fails
/// recovers fast, while a permanently broken task slows down quickly.
///
/// The restart log is `error!` for the first `LOUD_FAILURE_LIMIT` consecutive
/// failures, then downgrades to `warn!` so a broken task produces a single
/// loud ERROR followed by filterable noise.
///
/// The outer supervisor task itself never exits, so a supervised task cannot
/// silently die.
///
/// `factory` is called once per restart. It must produce a fresh future each
/// time, which means any state captured by the closure must be `Clone`-able.
///
/// # Usage
///
/// ```ignore
/// supervised("worker_loop", move || {
///     let services = services.clone();
///     async move { run_worker_loop_inner(services).await }
/// });
/// ```
///
/// # Important
///
/// MUST be used for every long-running task. Never spawn a forever-running
/// future directly with `tokio::spawn` — use this wrapper instead.
pub fn supervised<F, Fut>(name: &'static str, factory: F)
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut consecutive_failures: u32 = 0;
        let mut backoff = BASE_BACKOFF;

        loop {
            let started_at = Instant::now();
            let handle = tokio::spawn(factory());
            let outcome = handle.await;
            let ran_for = started_at.elapsed();

            // Healthy run — the task stayed up long enough that whatever just
            // happened is treated as a transient hiccup, not a persistent fault.
            let was_healthy = ran_for >= HEALTHY_RUN_THRESHOLD;
            if was_healthy {
                if consecutive_failures > 0 {
                    info!(
                        task = name,
                        previous_failures = consecutive_failures,
                        ran_for_seconds = ran_for.as_secs(),
                        "task was healthy before exit, resetting backoff"
                    );
                }
                consecutive_failures = 0;
                backoff = BASE_BACKOFF;
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
            }

            // Log the outcome. First few consecutive failures are ERROR; after
            // that, downgrade to WARN so a persistently broken task doesn't
            // flood ERROR logs forever.
            let loud = consecutive_failures <= LOUD_FAILURE_LIMIT;
            log_outcome(
                name,
                &outcome,
                consecutive_failures,
                ran_for.as_secs(),
                backoff.as_secs(),
                loud,
            );

            // Sleep with jitter, then grow the backoff for the next iteration.
            let jitter_ms = rand::rng().random_range(0..MAX_JITTER_MS);
            tokio::time::sleep(backoff + Duration::from_millis(jitter_ms)).await;

            // Only grow after a genuine failure. Healthy runs reset above.
            if !was_healthy {
                backoff = next_backoff(backoff);
            }
        }
    });
}

fn next_backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > MAX_BACKOFF {
        MAX_BACKOFF
    } else {
        doubled
    }
}

fn log_outcome(
    name: &'static str,
    outcome: &Result<anyhow::Result<()>, tokio::task::JoinError>,
    consecutive_failures: u32,
    ran_for_seconds: u64,
    next_backoff_seconds: u64,
    loud: bool,
) {
    match outcome {
        Ok(Ok(())) => {
            // Clean exit from a task that's supposed to run forever is still a
            // restart — a task shouldn't return `Ok(())`. Keep this at warn
            // regardless of backoff state.
            warn!(
                task = name,
                consecutive_failures,
                ran_for_seconds,
                next_backoff_seconds,
                "task exited cleanly, restarting"
            );
        }
        Ok(Err(e)) => {
            if loud {
                error!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task returned error, restarting"
                );
            } else {
                warn!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task returned error, restarting (backoff mode)"
                );
            }
        }
        Err(e) if e.is_panic() => {
            if loud {
                error!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task panicked, restarting"
                );
            } else {
                warn!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task panicked, restarting (backoff mode)"
                );
            }
        }
        Err(e) => {
            if loud {
                error!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task join error, restarting"
                );
            } else {
                warn!(
                    task = name,
                    error = ?e,
                    consecutive_failures,
                    ran_for_seconds,
                    next_backoff_seconds,
                    "task join error, restarting (backoff mode)"
                );
            }
        }
    }
}
