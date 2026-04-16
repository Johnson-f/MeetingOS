# Google Calendar Disconnect + Resync + Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Google Calendar disconnect fully clean up (watches, calendars, events), make resync return meaningful status (including auth_required), and add a status endpoint so the frontend can show connection health and prompt re-auth.

**Architecture:** Backend-first — add `stop_channel` to the Google client, add cleanup Turso methods, fix the disconnect/resync handlers, add a status endpoint, then update the job handler to stop retrying on dead tokens. Frontend — add a status hook, update the calendar sidebar to show connection state with disconnect/resync/reconnect buttons.

**Tech Stack:** Rust/Axum backend, libSQL/Turso DB, Google Calendar API v3, Next.js/React frontend with TanStack Query + sonner toasts.

---

## File Map

### Backend

| File | Action | Responsibility |
|------|--------|---------------|
| `src/service/google_calendar/client.rs` | Modify | Add `stop_channel()` method |
| `src/service/turso/read_operations/calendar.rs` | Modify | Add cleanup queries + status query |
| `src/routes/calendar.rs` | Modify | Rewrite `google_disconnect`, `google_resync`, add `google_status` |
| `src/routes/router.rs` | Modify | Wire new `google_status` route |
| `src/service/jobs/handlers.rs` | Modify | Fix `sync_google_calendar_job` to mark `auth_required` on token failure |

### Frontend

| File | Action | Responsibility |
|------|--------|---------------|
| `frontend/src/lib/backend_connection/client.ts` | Modify | Add `getGoogleCalendarStatus()` method |
| `frontend/src/lib/types/api.ts` | Modify | Add `GoogleCalendarStatusResponse` type |
| `frontend/src/lib/hooks/use-google-calendar.ts` | Modify | Add `useGoogleCalendarStatus` hook, update resync hook |
| `frontend/src/components/schedule/calendar-view.tsx` | Modify | Replace static connect button with dynamic connection section |

---

## Task 1: Add `stop_channel` to Google Calendar client

**Files:**
- Modify: `src/service/google_calendar/client.rs:122-130`

- [ ] **Step 1: Add `stop_channel` method**

Add after the existing `revoke_token` method (line 130):

```rust
/// Stop a push notification channel (best-effort, used on disconnect)
pub async fn stop_channel(
    &self,
    access_token: &str,
    channel_id: &str,
    resource_id: &str,
) -> Result<()> {
    self.http
        .post(format!("{}/channels/stop", CALENDAR_API_BASE))
        .bearer_auth(access_token)
        .json(&serde_json::json!({
            "id": channel_id,
            "resourceId": resource_id,
        }))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors related to `stop_channel`

- [ ] **Step 3: Commit**

```bash
git add src/service/google_calendar/client.rs
git commit -m "feat: add stop_channel to Google Calendar client"
```

---

## Task 2: Add cleanup + status Turso methods

**Files:**
- Modify: `src/service/turso/read_operations/calendar.rs`

- [ ] **Step 1: Add `CalendarWatch` struct and `get_calendar_watches` method**

Add the struct after `StoredCalendarEvent` (after line 28):

```rust
#[derive(Debug, Clone)]
pub struct CalendarWatch {
    pub provider_calendar_id: String,
    pub watch_channel_id: String,
    pub watch_resource_id: String,
}
```

Add the method inside the `impl TursoClient` block (after `list_calendar_calendars`, around line 164):

```rust
pub async fn get_calendar_watches(
    &self,
    oauth_connection_id: &str,
) -> Result<Vec<CalendarWatch>> {
    let conn = self.connection().await?;
    let mut rows = conn
        .query(
            "SELECT provider_calendar_id, watch_channel_id, watch_resource_id FROM calendar_calendars WHERE oauth_connection_id = ? AND watch_channel_id IS NOT NULL AND watch_resource_id IS NOT NULL",
            params![oauth_connection_id],
        )
        .await?;

    let mut watches = Vec::new();
    while let Some(row) = rows.next().await? {
        watches.push(CalendarWatch {
            provider_calendar_id: row.get::<String>(0)?,
            watch_channel_id: row.get::<String>(1)?,
            watch_resource_id: row.get::<String>(2)?,
        });
    }
    Ok(watches)
}
```

- [ ] **Step 2: Add `delete_calendars_for_connection` and `delete_events_for_connection`**

Add inside the `impl TursoClient` block (after `get_calendar_watches`):

```rust
pub async fn delete_events_for_connection(&self, oauth_connection_id: &str) -> Result<u64> {
    let conn = self.connection().await?;
    let result = conn
        .execute(
            "DELETE FROM calendar_events WHERE calendar_id IN (SELECT id FROM calendar_calendars WHERE oauth_connection_id = ?)",
            params![oauth_connection_id],
        )
        .await?;
    Ok(result)
}

pub async fn delete_calendars_for_connection(&self, oauth_connection_id: &str) -> Result<u64> {
    let conn = self.connection().await?;
    let result = conn
        .execute(
            "DELETE FROM calendar_calendars WHERE oauth_connection_id = ?",
            params![oauth_connection_id],
        )
        .await?;
    Ok(result)
}
```

- [ ] **Step 3: Add `update_oauth_connection_status` method**

Add inside the `impl TursoClient` block (after the new delete methods):

```rust
pub async fn update_oauth_connection_status(
    &self,
    connection_id: &str,
    status: &str,
) -> Result<()> {
    let conn = self.connection().await?;
    conn.execute(
        "UPDATE oauth_connections SET status = ?, updated_at = ? WHERE id = ?",
        (status, now_rfc3339(), connection_id),
    )
    .await?;
    Ok(())
}
```

- [ ] **Step 4: Add `GoogleCalendarStatus` struct and `get_google_calendar_status` method**

Add the struct after `CalendarWatch`:

```rust
#[derive(Debug, Clone)]
pub struct GoogleCalendarStatus {
    pub connected: bool,
    pub status: String,
    pub calendars_count: i64,
    pub last_synced_at: Option<String>,
    pub connected_at: Option<String>,
}
```

Add the method inside the `impl TursoClient` block:

```rust
pub async fn get_google_calendar_status(
    &self,
    user_id: &str,
) -> Result<Option<GoogleCalendarStatus>> {
    let conn = self.connection().await?;
    let mut rows = conn
        .query(
            r#"
            SELECT oc.status, oc.created_at,
                   COUNT(cc.id) as calendars_count,
                   MAX(cc.last_synced_at) as last_synced_at
            FROM oauth_connections oc
            LEFT JOIN calendar_calendars cc ON cc.oauth_connection_id = oc.id AND cc.status = 'active'
            WHERE oc.user_id = ? AND oc.provider = 'google' AND oc.status != 'disconnected'
            GROUP BY oc.id
            LIMIT 1
            "#,
            params![user_id],
        )
        .await?;

    if let Some(row) = rows.next().await? {
        let status: String = row.get(0)?;
        return Ok(Some(GoogleCalendarStatus {
            connected: status == "connected" || status == "auth_required",
            status,
            calendars_count: row.get::<i64>(2)?,
            last_synced_at: row.get::<Option<String>>(3)?,
            connected_at: row.get::<Option<String>>(1)?,
        }));
    }
    Ok(None)
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
git add src/service/turso/read_operations/calendar.rs
git commit -m "feat: add cleanup and status query methods to Turso calendar"
```

---

## Task 3: Rewrite `google_disconnect` handler

**Files:**
- Modify: `src/routes/calendar.rs:200-239`

- [ ] **Step 1: Replace the `google_disconnect` handler**

Replace the existing `google_disconnect` function (lines 201-239) with:

```rust
/// POST /api/v1/calendar/google/disconnect
pub async fn google_disconnect(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<StatusCode, ApiError> {
    info!(sub = %jwt.sub, "POST /api/v1/calendar/google/disconnect");
    let user = current_user(&state, &jwt).await?;

    let google = state.services.google_calendar.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Google Calendar is not configured",
        )
    })?;

    // Find the user's Google OAuth connection
    let connection = state
        .services
        .turso
        .get_oauth_connection(&user.user_id, "google")
        .await?
        .ok_or_else(|| {
            ApiError::new(StatusCode::NOT_FOUND, "no Google Calendar connection found")
        })?;

    let access_token = connection.access_token.as_deref().unwrap_or_default();

    // 1. Stop webhook watches (best-effort)
    let watches = state
        .services
        .turso
        .get_calendar_watches(&connection.id)
        .await
        .unwrap_or_default();
    for watch in &watches {
        if let Err(e) = google
            .stop_channel(access_token, &watch.watch_channel_id, &watch.watch_resource_id)
            .await
        {
            warn!(
                channel = %watch.watch_channel_id,
                error = %e,
                "failed to stop calendar watch (best-effort)"
            );
        }
    }

    // 2. Revoke the refresh token (preferred) or access token
    if let Some(refresh_token) = &connection.refresh_token {
        let _ = google.revoke_token(refresh_token).await;
    } else if let Some(token) = &connection.access_token {
        let _ = google.revoke_token(token).await;
    }

    // 3. Delete calendar events
    let events_deleted = state
        .services
        .turso
        .delete_events_for_connection(&connection.id)
        .await
        .unwrap_or(0);

    // 4. Delete calendars
    let calendars_deleted = state
        .services
        .turso
        .delete_calendars_for_connection(&connection.id)
        .await
        .unwrap_or(0);

    // 5. Soft-delete the connection
    state
        .services
        .turso
        .delete_oauth_connection(&connection.id)
        .await?;

    info!(
        user_id = %user.user_id,
        events_deleted = events_deleted,
        calendars_deleted = calendars_deleted,
        watches_stopped = watches.len(),
        "Google Calendar disconnected with full cleanup"
    );
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/routes/calendar.rs
git commit -m "feat: disconnect handler now cleans up watches, events, calendars"
```

---

## Task 4: Rewrite `google_resync` handler

**Files:**
- Modify: `src/routes/calendar.rs:242-273`

- [ ] **Step 1: Replace the `google_resync` handler**

Replace the existing `google_resync` function (lines 242-273) with:

```rust
/// POST /api/v1/calendar/google/resync — manually trigger a resync
pub async fn google_resync(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<Json<serde_json::Value>, ApiError> {
    info!(sub = %jwt.sub, "POST /api/v1/calendar/google/resync");
    let user = current_user(&state, &jwt).await?;

    let google = state.services.google_calendar.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Google Calendar is not configured",
        )
    })?;

    let connection = state
        .services
        .turso
        .get_oauth_connection(&user.user_id, "google")
        .await?
        .ok_or_else(|| {
            ApiError::new(StatusCode::NOT_FOUND, "no Google Calendar connection found")
        })?;

    // Validate the token before queuing a job
    let refresh_token = connection.refresh_token.as_deref().ok_or_else(|| {
        ApiError::new(StatusCode::CONFLICT, "no refresh token available")
    })?;

    match google.refresh_token(refresh_token).await {
        Ok(tokens) => {
            // Token is valid — update stored tokens and enqueue sync
            state
                .services
                .turso
                .update_oauth_tokens(
                    &connection.id,
                    &tokens.access_token,
                    tokens.refresh_token.as_deref(),
                )
                .await?;

            // Ensure connection is marked as connected (in case it was auth_required)
            state
                .services
                .turso
                .update_oauth_connection_status(&connection.id, "connected")
                .await?;

            state
                .services
                .turso
                .enqueue_job(
                    "sync_google_calendar",
                    Some(&format!("resync-{}", connection.id)),
                    &json!({
                        "oauth_connection_id": connection.id,
                        "user_id": user.user_id,
                        "workspace_id": user.workspace_id,
                    }),
                )
                .await?;

            Ok(Json(json!({ "status": "sync_queued" })))
        }
        Err(e) => {
            warn!(
                user_id = %user.user_id,
                error = %e,
                "resync failed: Google token refresh rejected"
            );

            // Mark connection as needing re-auth
            state
                .services
                .turso
                .update_oauth_connection_status(&connection.id, "auth_required")
                .await?;

            Ok(Json(json!({ "status": "auth_required" })))
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/routes/calendar.rs
git commit -m "feat: resync validates token first, returns auth_required when dead"
```

---

## Task 5: Add `google_status` endpoint

**Files:**
- Modify: `src/routes/calendar.rs` (add new handler at end)
- Modify: `src/routes/router.rs` (wire the route)

- [ ] **Step 1: Add `google_status` handler**

Add at the end of `src/routes/calendar.rs` (before the final closing, after the `google_resync` function):

```rust
/// GET /api/v1/calendar/google/status — connection health check
pub async fn google_status(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = current_user(&state, &jwt).await?;

    match state
        .services
        .turso
        .get_google_calendar_status(&user.user_id)
        .await?
    {
        Some(status) => Ok(Json(json!({
            "connected": status.connected,
            "status": status.status,
            "calendars_count": status.calendars_count,
            "last_synced_at": status.last_synced_at,
            "connected_at": status.connected_at,
        }))),
        None => Ok(Json(json!({
            "connected": false,
        }))),
    }
}
```

- [ ] **Step 2: Wire the route in router.rs**

In `src/routes/router.rs`, add `google_status` to the import from `calendar`:

Change:
```rust
use super::{
    ...
    calendar::{google_callback, google_connect, google_disconnect, google_resync},
```

To:
```rust
use super::{
    ...
    calendar::{google_callback, google_connect, google_disconnect, google_resync, google_status},
```

Add the route in the protected_routes section, after the `google_resync` route:

```rust
        .route("/api/v1/calendar/google/status", get(google_status))
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/routes/calendar.rs src/routes/router.rs
git commit -m "feat: add GET /api/v1/calendar/google/status endpoint"
```

---

## Task 6: Fix `sync_google_calendar_job` to stop retrying dead tokens

**Files:**
- Modify: `src/service/jobs/handlers.rs:615-676`

- [ ] **Step 1: Replace the sync job handler**

Replace `sync_google_calendar_job` (lines 615-676) with:

```rust
pub(super) async fn sync_google_calendar_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let oauth_connection_id = payload
        .get("oauth_connection_id")
        .and_then(Value::as_str)
        .context("missing oauth_connection_id")?;
    let user_id = payload
        .get("user_id")
        .and_then(Value::as_str)
        .context("missing user_id")?;
    let workspace_id = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .context("missing workspace_id")?;

    info!(oauth_connection_id = %oauth_connection_id, "syncing Google Calendar");

    // Get the access token (and refresh if needed)
    let connection = services
        .turso
        .get_oauth_connection(user_id, "google")
        .await?
        .context("OAuth connection not found")?;

    let mut access_token = connection.access_token.clone().unwrap_or_default();

    // Try to refresh if we have a refresh token
    if let (Some(google), Some(refresh_token)) =
        (&services.google_calendar, &connection.refresh_token)
    {
        match google.refresh_token(refresh_token).await {
            Ok(tokens) => {
                access_token = tokens.access_token.clone();
                services
                    .turso
                    .update_oauth_tokens(
                        &connection.id,
                        &tokens.access_token,
                        tokens.refresh_token.as_deref(),
                    )
                    .await?;
            }
            Err(e) => {
                warn!(
                    oauth_connection_id = %oauth_connection_id,
                    error = %e,
                    "Google token refresh failed, marking connection as auth_required"
                );
                services
                    .turso
                    .update_oauth_connection_status(&connection.id, "auth_required")
                    .await?;
                // Return Ok to prevent retries — the token is dead, retrying won't help
                return Ok(());
            }
        }
    }

    crate::service::google_calendar::sync::sync_user_calendar(
        services,
        oauth_connection_id,
        &access_token,
        user_id,
        workspace_id,
    )
    .await?;

    info!(oauth_connection_id = %oauth_connection_id, "Google Calendar sync complete");
    Ok(())
}
```

Key change: when token refresh fails, we mark the connection `auth_required` and return `Ok(())` so the job system doesn't retry.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/service/jobs/handlers.rs
git commit -m "fix: sync job marks connection auth_required on dead token instead of retrying"
```

---

## Task 7: Also skip `auth_required` connections in periodic scheduler

**Files:**
- Modify: `src/service/turso/read_operations/calendar.rs` (update `get_all_active_oauth_connections`)

- [ ] **Step 1: Verify current query**

The current query in `get_all_active_oauth_connections` (line 82) filters `WHERE provider = ? AND status = 'connected'`. This already excludes `auth_required` connections because it only matches `connected`. No change needed — already correct.

But we should also verify `get_oauth_connection` (used by the resync and disconnect handlers). The current query (line 58) filters `WHERE user_id = ? AND provider = ? AND status = 'connected'`. This means if a connection is `auth_required`, the user can't disconnect or resync it — they'd get a 404.

- [ ] **Step 2: Update `get_oauth_connection` to also find `auth_required` connections**

In `src/service/turso/read_operations/calendar.rs`, change the query in `get_oauth_connection` (around line 58):

From:
```rust
"SELECT id, user_id, workspace_id, access_token_encrypted, refresh_token_encrypted FROM oauth_connections WHERE user_id = ? AND provider = ? AND status = 'connected' LIMIT 1",
```

To:
```rust
"SELECT id, user_id, workspace_id, access_token_encrypted, refresh_token_encrypted FROM oauth_connections WHERE user_id = ? AND provider = ? AND status IN ('connected', 'auth_required') LIMIT 1",
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/service/turso/read_operations/calendar.rs
git commit -m "fix: get_oauth_connection also finds auth_required connections"
```

---

## Task 8: Frontend — add status client method and type

**Files:**
- Modify: `frontend/src/lib/types/api.ts`
- Modify: `frontend/src/lib/backend_connection/client.ts`

- [ ] **Step 1: Add `GoogleCalendarStatusResponse` type**

Add at the end of `frontend/src/lib/types/api.ts`:

```typescript
export interface GoogleCalendarStatusResponse {
  connected: boolean;
  status?: "connected" | "auth_required" | "disconnected";
  calendars_count?: number;
  last_synced_at?: string | null;
  connected_at?: string | null;
}
```

- [ ] **Step 2: Add `getGoogleCalendarStatus` to the client**

In `frontend/src/lib/backend_connection/client.ts`, add the import of the new type at the top (line 1-19):

Add `GoogleCalendarStatusResponse` to the type import.

Then add the method after `resyncGoogleCalendar()` (around line 181):

```typescript
  getGoogleCalendarStatus() {
    return this.request<GoogleCalendarStatusResponse>(
      "/api/v1/calendar/google/status"
    );
  }
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/types/api.ts frontend/src/lib/backend_connection/client.ts
git commit -m "feat: add Google Calendar status type and client method"
```

---

## Task 9: Frontend — add `useGoogleCalendarStatus` hook and update resync hook

**Files:**
- Modify: `frontend/src/lib/hooks/use-google-calendar.ts`

- [ ] **Step 1: Replace the entire file**

Replace `frontend/src/lib/hooks/use-google-calendar.ts` with:

```typescript
"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useGoogleCalendarConnect() {
  const client = useBackendClient();

  return useMutation({
    mutationFn: async () => {
      const { url } = await client.getGoogleCalendarConnectUrl();
      window.location.href = url;
    },
  });
}

export function useGoogleCalendarDisconnect() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => client.disconnectGoogleCalendar(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["backend", "current-user"] });
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
      queryClient.invalidateQueries({
        queryKey: ["backend", "google-calendar-status"],
      });
    },
  });
}

export function useGoogleCalendarResync() {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => client.resyncGoogleCalendar(),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ["backend", "google-calendar-status"],
      });
      queryClient.invalidateQueries({ queryKey: ["backend", "meetings"] });
    },
  });
}

export function useGoogleCalendarStatus() {
  const client = useBackendClient();

  return useQuery({
    queryKey: ["backend", "google-calendar-status"],
    queryFn: () => client.getGoogleCalendarStatus(),
    staleTime: 30_000,
  });
}
```

- [ ] **Step 2: Commit**

```bash
git add frontend/src/lib/hooks/use-google-calendar.ts
git commit -m "feat: add useGoogleCalendarStatus hook, update resync/disconnect invalidation"
```

---

## Task 10: Frontend — update calendar sidebar with connection management UI

**Files:**
- Modify: `frontend/src/components/schedule/calendar-view.tsx`

- [ ] **Step 1: Update imports**

Replace the current google calendar import (line 18):

```typescript
import { useGoogleCalendarConnect } from "@/lib/hooks/use-google-calendar"
```

With:

```typescript
import {
  useGoogleCalendarConnect,
  useGoogleCalendarDisconnect,
  useGoogleCalendarResync,
  useGoogleCalendarStatus,
} from "@/lib/hooks/use-google-calendar"
import { toast } from "sonner"
```

- [ ] **Step 2: Extract the calendar connection section into a component**

Add a new component before the `CalendarView` function (before line 267). This replaces the static "Connect a Calendar" section:

```tsx
function GoogleCalendarSection() {
  const googleConnect = useGoogleCalendarConnect()
  const googleDisconnect = useGoogleCalendarDisconnect()
  const googleResync = useGoogleCalendarResync()
  const { data: status, isLoading } = useGoogleCalendarStatus()

  function handleDisconnect() {
    if (!confirm("Disconnect Google Calendar? This will remove all synced calendar data.")) return
    googleDisconnect.mutate(undefined, {
      onSuccess: () => toast.success("Google Calendar disconnected"),
      onError: () => toast.error("Failed to disconnect"),
    })
  }

  function handleResync() {
    googleResync.mutate(undefined, {
      onSuccess: (data) => {
        if (data.status === "auth_required") {
          toast.error("Session expired — please reconnect your Google account")
        } else {
          toast.success("Calendar sync started")
        }
      },
      onError: () => toast.error("Failed to sync"),
    })
  }

  if (isLoading) {
    return (
      <div className="flex flex-col gap-2 mt-auto pt-4">
        <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
          Calendar
        </span>
        <Skeleton className="h-8 w-full" />
      </div>
    )
  }

  // Not connected
  if (!status?.connected) {
    return (
      <div className="flex flex-col gap-2 mt-auto pt-4">
        <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
          Connect a Calendar
        </span>
        <Button
          variant="outline"
          size="sm"
          className="justify-start"
          onClick={() => googleConnect.mutate()}
          disabled={googleConnect.isPending}
        >
          <HugeiconsIcon icon={Calendar01Icon} strokeWidth={2} className="size-4 mr-2" />
          Google
        </Button>
      </div>
    )
  }

  // Connected but auth_required
  if (status.status === "auth_required") {
    return (
      <div className="flex flex-col gap-2 mt-auto pt-4">
        <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
          Google Calendar
        </span>
        <Badge variant="outline" className="text-amber-600 w-fit text-[0.6rem]">
          Reconnect required
        </Badge>
        <Button
          variant="outline"
          size="sm"
          className="justify-start"
          onClick={() => googleConnect.mutate()}
          disabled={googleConnect.isPending}
        >
          <HugeiconsIcon icon={Calendar01Icon} strokeWidth={2} className="size-4 mr-2" />
          Reconnect Google
        </Button>
        <button
          onClick={handleDisconnect}
          disabled={googleDisconnect.isPending}
          className="text-[0.65rem] text-muted-foreground hover:text-destructive transition-colors text-left"
        >
          {googleDisconnect.isPending ? "Disconnecting..." : "Disconnect"}
        </button>
      </div>
    )
  }

  // Connected and healthy
  return (
    <div className="flex flex-col gap-2 mt-auto pt-4">
      <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
        Google Calendar
      </span>
      <div className="flex items-center gap-1.5">
        <span className="size-1.5 rounded-full bg-emerald-500" />
        <span className="text-xs text-muted-foreground">Connected</span>
        {status.calendars_count != null && (
          <span className="text-[0.6rem] text-muted-foreground/60">
            · {status.calendars_count} calendar{status.calendars_count !== 1 ? "s" : ""}
          </span>
        )}
      </div>
      {status.last_synced_at && (
        <span className="text-[0.6rem] text-muted-foreground/60">
          Last synced {new Date(status.last_synced_at).toLocaleString()}
        </span>
      )}
      <Button
        variant="outline"
        size="sm"
        className="justify-start"
        onClick={handleResync}
        disabled={googleResync.isPending}
      >
        {googleResync.isPending ? "Syncing..." : "Re-sync now"}
      </Button>
      <button
        onClick={handleDisconnect}
        disabled={googleDisconnect.isPending}
        className="text-[0.65rem] text-muted-foreground hover:text-destructive transition-colors text-left"
      >
        {googleDisconnect.isPending ? "Disconnecting..." : "Disconnect"}
      </button>
    </div>
  )
}
```

- [ ] **Step 3: Replace the sidebar calendar section in `CalendarView`**

In the `CalendarView` component, replace the "Connect a Calendar" sidebar section (lines 337-351):

From:
```tsx
        {/* Connect calendar */}
        <div className="flex flex-col gap-2 mt-auto pt-4">
          <span className="text-[0.65rem] font-medium text-muted-foreground uppercase tracking-wider">
            Connect a Calendar
          </span>
          <Button
            variant="outline"
            size="sm"
            className="justify-start"
            onClick={() => googleConnect.mutate()}
            disabled={googleConnect.isPending}
          >
            <HugeiconsIcon icon={Calendar01Icon} strokeWidth={2} className="size-4 mr-2" />
            Google
          </Button>
        </div>
```

To:
```tsx
        <GoogleCalendarSection />
```

Also remove the now-unused `googleConnect` variable from `CalendarView` (line 277):

Remove this line:
```tsx
  const googleConnect = useGoogleCalendarConnect()
```

- [ ] **Step 4: Verify the frontend builds**

Run: `cd /Users/user/meeting-bot/frontend && bun run build 2>&1 | tail -10`
Expected: build succeeds

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/schedule/calendar-view.tsx
git commit -m "feat: calendar sidebar shows connection status with disconnect/resync/reconnect"
```

---

## Task 11: Build and verify both backend and frontend

- [ ] **Step 1: Cargo build**

Run: `cd /Users/user/meeting-bot && cargo build 2>&1 | tail -10`
Expected: build succeeds

- [ ] **Step 2: Frontend build**

Run: `cd /Users/user/meeting-bot/frontend && bun run build 2>&1 | tail -10`
Expected: build succeeds

- [ ] **Step 3: Final commit if any fixes needed**

If any compile errors were found and fixed, commit the fixes.
