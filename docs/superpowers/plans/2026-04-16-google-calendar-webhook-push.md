# Google Calendar Webhook Push Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 15-minute periodic polling with Google's push notifications for calendar sync, and add automatic watch renewal so the system is self-maintaining.

**Architecture:** Google sends a POST to our webhook when a calendar changes. We look up which connection owns the watch channel, enqueue a sync job, and return 200. A 6-hour ticker renews watches before they expire. The existing `sync_google_calendar` job + `sync_user_calendar` logic is reused unchanged.

**Tech Stack:** Rust/Axum backend, libSQL/Turso DB, Google Calendar API v3 push notifications.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/service/turso/read_operations/calendar.rs` | Modify | Add `get_connection_by_watch_channel`, `get_expiring_watches` queries |
| `src/routes/webhooks.rs` | Modify | Add `google_calendar_webhook` handler |
| `src/routes/router.rs` | Modify | Wire webhook handler, update imports |
| `src/service/jobs/runner.rs` | Modify | Remove sync_ticker, add watch_renewal_ticker |

---

## Task 1: Add `get_connection_by_watch_channel` Turso method

**Files:**
- Modify: `src/service/turso/read_operations/calendar.rs`

- [ ] **Step 1: Add the method inside the `impl TursoClient` block**

Add after the existing `get_calendar_watches` method (around line 200):

```rust
pub async fn get_connection_by_watch_channel(
    &self,
    channel_id: &str,
) -> Result<Option<StoredOAuthConnection>> {
    let conn = self.connection().await?;
    let mut rows = conn
        .query(
            r#"
            SELECT oc.id, oc.user_id, oc.workspace_id, oc.access_token_encrypted, oc.refresh_token_encrypted
            FROM calendar_calendars cc
            JOIN oauth_connections oc ON oc.id = cc.oauth_connection_id
            WHERE cc.watch_channel_id = ?
              AND oc.status IN ('connected', 'auth_required')
            LIMIT 1
            "#,
            params![channel_id],
        )
        .await?;

    if let Some(row) = rows.next().await? {
        return Ok(Some(StoredOAuthConnection {
            id: row.get::<String>(0)?,
            user_id: row.get::<String>(1)?,
            workspace_id: row.get::<String>(2)?,
            access_token: row.get::<Option<String>>(3)?,
            refresh_token: row.get::<Option<String>>(4)?,
        }));
    }
    Ok(None)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 2: Add `get_expiring_watches` Turso method

**Files:**
- Modify: `src/service/turso/read_operations/calendar.rs`

- [ ] **Step 1: Add `ExpiringWatch` struct after `GoogleCalendarStatus`**

```rust
#[derive(Debug, Clone)]
pub struct ExpiringWatch {
    pub oauth_connection_id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub provider_calendar_id: String,
    pub watch_channel_id: String,
    pub watch_resource_id: String,
}
```

- [ ] **Step 2: Add the method inside `impl TursoClient`**

Add after `get_connection_by_watch_channel`:

```rust
/// Find watches expiring within `hours` hours, for connected accounts only.
pub async fn get_expiring_watches(&self, hours: i64) -> Result<Vec<ExpiringWatch>> {
    let conn = self.connection().await?;
    let threshold_ms = (chrono::Utc::now() + chrono::Duration::hours(hours))
        .timestamp_millis()
        .to_string();

    let mut rows = conn
        .query(
            r#"
            SELECT cc.oauth_connection_id, oc.user_id, oc.workspace_id,
                   oc.access_token_encrypted, oc.refresh_token_encrypted,
                   cc.provider_calendar_id, cc.watch_channel_id, cc.watch_resource_id
            FROM calendar_calendars cc
            JOIN oauth_connections oc ON oc.id = cc.oauth_connection_id
            WHERE cc.watch_channel_id IS NOT NULL
              AND cc.watch_expires_at IS NOT NULL
              AND CAST(cc.watch_expires_at AS INTEGER) < CAST(? AS INTEGER)
              AND oc.status = 'connected'
            "#,
            params![threshold_ms],
        )
        .await?;

    let mut watches = Vec::new();
    while let Some(row) = rows.next().await? {
        watches.push(ExpiringWatch {
            oauth_connection_id: row.get::<String>(0)?,
            user_id: row.get::<String>(1)?,
            workspace_id: row.get::<String>(2)?,
            access_token: row.get::<Option<String>>(3)?,
            refresh_token: row.get::<Option<String>>(4)?,
            provider_calendar_id: row.get::<String>(5)?,
            watch_channel_id: row.get::<String>(6)?,
            watch_resource_id: row.get::<String>(7)?,
        });
    }
    Ok(watches)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 3: Implement `google_calendar_webhook` handler

**Files:**
- Modify: `src/routes/webhooks.rs`

- [ ] **Step 1: Add the webhook handler**

Add at the end of `src/routes/webhooks.rs`, after the `recall_webhook` function:

```rust
use tracing::warn;

/// POST /api/v1/webhooks/google-calendar — Google Calendar push notification
pub async fn google_calendar_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> StatusCode {
    let channel_id = headers
        .get("x-goog-channel-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let resource_state = headers
        .get("x-goog-resource-state")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    info!(
        channel_id = %channel_id,
        resource_state = %resource_state,
        "Google Calendar webhook received"
    );

    // "sync" is the initial handshake when a watch is first registered
    if resource_state == "sync" {
        return StatusCode::OK;
    }

    // "exists" means something changed in the calendar
    if resource_state != "exists" {
        info!(resource_state = %resource_state, "ignoring unknown resource state");
        return StatusCode::OK;
    }

    if channel_id.is_empty() {
        warn!("Google Calendar webhook missing channel ID");
        return StatusCode::OK;
    }

    // Look up which connection owns this watch channel
    let connection = match state
        .services
        .turso
        .get_connection_by_watch_channel(channel_id)
        .await
    {
        Ok(Some(conn)) => conn,
        Ok(None) => {
            warn!(channel_id = %channel_id, "no connection found for watch channel");
            return StatusCode::OK;
        }
        Err(e) => {
            warn!(channel_id = %channel_id, error = %e, "failed to look up watch channel");
            return StatusCode::OK;
        }
    };

    // Enqueue a sync job (deduped so rapid-fire notifications don't spam)
    let _ = state
        .services
        .turso
        .enqueue_job(
            "sync_google_calendar",
            Some(&format!("webhook-sync-{}", channel_id)),
            &json!({
                "oauth_connection_id": connection.id,
                "user_id": connection.user_id,
                "workspace_id": connection.workspace_id,
            }),
        )
        .await;

    info!(
        channel_id = %channel_id,
        connection_id = %connection.id,
        "enqueued calendar sync from webhook"
    );

    StatusCode::OK
}
```

Note: This handler always returns 200. If we return an error, Google retries with exponential backoff which makes things worse.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 4: Wire the webhook in the router

**Files:**
- Modify: `src/routes/router.rs`

- [ ] **Step 1: Add the import**

Change the webhooks import (line 25):

From:
```rust
    webhooks::recall_webhook,
```

To:
```rust
    webhooks::{google_calendar_webhook, recall_webhook},
```

- [ ] **Step 2: Replace the `not_implemented` route**

Change line 36:

From:
```rust
        .route("/api/v1/webhooks/google-calendar", post(not_implemented))
```

To:
```rust
        .route("/api/v1/webhooks/google-calendar", post(google_calendar_webhook))
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 5: Remove periodic sync, add watch renewal ticker

**Files:**
- Modify: `src/service/jobs/runner.rs`

- [ ] **Step 1: Replace `run_periodic_scheduler` function**

Replace the entire `run_periodic_scheduler` function (lines 107-163) with:

```rust
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
```

- [ ] **Step 2: Add `renew_expiring_watches` function**

Add after `run_periodic_scheduler`, before `log_worker_shutdown`:

```rust
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
```

- [ ] **Step 3: Remove unused imports**

In the imports at the top of `runner.rs` (lines 10-14), remove `JOB_SYNC_GOOGLE_CALENDAR` from the constants import since the periodic scheduler no longer enqueues it directly. The constant is still used by the job dispatch match in `process_job`, so keep it if it's referenced there.

Check: `JOB_SYNC_GOOGLE_CALENDAR` is still used in the `process_job` match (line 96), so it stays in the import. No import changes needed.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 6: Verify full build

- [ ] **Step 1: Cargo build**

Run: `cargo build 2>&1 | tail -10`
Expected: build succeeds

- [ ] **Step 2: Review the changes**

Quick check that:
- `not_implemented` is still imported (used by Microsoft Graph webhook on line 38)
- `JOB_SYNC_GOOGLE_CALENDAR` is still imported (used in `process_job` match)
- No dead code warnings from our changes
