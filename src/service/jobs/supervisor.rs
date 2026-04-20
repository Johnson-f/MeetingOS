use std::future::Future;
use std::time::Duration;

use rand::Rng;
use tracing::{error, warn};

/// Supervise a long-running background task.
///
/// The supervisor awaits the inner `JoinHandle` and restarts the task on:
///   - panic
///   - `Err` return
///   - unexpected clean exit (a task that's supposed to run forever returning `Ok(())`)
///
/// Between restarts it sleeps 3-5 seconds with jitter. The outer supervisor task
/// itself never exits, so a supervised task cannot silently die.
///
/// `factory` is called once per restart. It must produce a fresh future each time,
/// which means any state captured by the closure must be `Clone`-able.
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
/// MUST be used for every long-running task. Never spawn a forever-running future
/// directly with `tokio::spawn` — use this wrapper instead.
pub fn supervised<F, Fut>(name: &'static str, factory: F)
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            let handle = tokio::spawn(factory());
            match handle.await {
                Ok(Ok(())) => warn!(task = name, "task exited cleanly, restarting"),
                Ok(Err(e)) => {
                    error!(task = name, error = ?e, "task returned error, restarting")
                }
                Err(e) if e.is_panic() => {
                    error!(task = name, error = ?e, "task panicked, restarting")
                }
                Err(e) => {
                    error!(task = name, error = ?e, "task join error, restarting")
                }
            }
            let jitter_ms = rand::rng().random_range(0..2000);
            tokio::time::sleep(Duration::from_millis(3_000 + jitter_ms)).await;
        }
    });
}
