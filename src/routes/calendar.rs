use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::models::ApiError;
use crate::service::turso::client::{new_id, now_rfc3339};

use super::{helpers::current_user, state::AppState};

/// GET /api/v1/calendar/google/connect — redirect user to Google OAuth
pub async fn google_connect(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<Response, ApiError> {
    info!(sub = %jwt.sub, "GET /api/v1/calendar/google/connect");
    let user = current_user(&state, &jwt).await?;
    let google = state.services.google_calendar.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Google Calendar is not configured",
        )
    })?;

    // Use user_id as state parameter for CSRF + user identification
    let state_param = format!("{}:{}", user.user_id, user.workspace_id);
    let auth_url = google.auth_url(&state_param);

    Ok(Json(json!({ "url": auth_url })).into_response())
}

#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// GET /api/v1/calendar/google/callback — handle OAuth callback
pub async fn google_callback(
    State(state): State<AppState>,
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Response, ApiError> {
    info!("GET /api/v1/calendar/google/callback");

    if let Some(error) = &query.error {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("Google OAuth error: {}", error),
        ));
    }

    let code = query
        .code
        .as_deref()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing authorization code"))?;

    let state_param = query
        .state
        .as_deref()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing state parameter"))?;

    // Parse user_id and workspace_id from state
    let parts: Vec<&str> = state_param.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid state parameter",
        ));
    }
    let user_id = parts[0];
    let workspace_id = parts[1];

    let google = state.services.google_calendar.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Google Calendar is not configured",
        )
    })?;

    // Exchange code for tokens
    let tokens = google.exchange_code(code).await.map_err(|e| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("token exchange failed: {}", e),
        )
    })?;

    let now = now_rfc3339();
    let connection_id = new_id();

    // Store OAuth connection
    state
        .services
        .turso
        .store_oauth_connection(
            &connection_id,
            workspace_id,
            user_id,
            "google",
            &tokens.access_token,
            tokens.refresh_token.as_deref(),
            &now,
        )
        .await?;

    // Fetch and store calendars
    let calendars = google
        .list_calendars(&tokens.access_token)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to list calendars: {}", e),
            )
        })?;

    for cal in &calendars {
        state
            .services
            .turso
            .store_calendar(
                &connection_id,
                "google",
                &cal.id,
                &cal.summary,
                cal.primary.unwrap_or(false),
            )
            .await?;
    }

    info!(
        user_id = %user_id,
        calendars = calendars.len(),
        "Google Calendar connected"
    );

    // Enqueue initial sync job
    state
        .services
        .turso
        .enqueue_job(
            "sync_google_calendar",
            Some(&format!("initial-sync-{}", connection_id)),
            &json!({
                "oauth_connection_id": connection_id,
                "user_id": user_id,
                "workspace_id": workspace_id,
            }),
        )
        .await?;

    // Register webhook watch for each calendar
    if let Some(public_url) = &state.config.public_app_url {
        let webhook_url = format!("{}/api/v1/webhooks/google-calendar", public_url);
        for cal in &calendars {
            let channel_id = new_id();
            match google
                .watch_calendar(&tokens.access_token, &cal.id, &channel_id, &webhook_url)
                .await
            {
                Ok(watch) => {
                    state
                        .services
                        .turso
                        .update_calendar_watch(
                            &connection_id,
                            &cal.id,
                            &watch.channel_id,
                            &watch.resource_id,
                            &watch.expiration,
                        )
                        .await?;
                    info!(calendar = %cal.id, channel = %watch.channel_id, "registered watch");
                }
                Err(e) => {
                    warn!(calendar = %cal.id, error = %e, "failed to register calendar watch");
                }
            }
        }
    }

    // Redirect back to the frontend
    let frontend_url = state
        .config
        .cors_allowed_origins
        .first()
        .cloned()
        .unwrap_or_else(|| "http://localhost:3000".to_owned());

    Ok(Redirect::to(&format!("{}/dashboard?google_connected=true", frontend_url)).into_response())
}

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
            .stop_channel(
                access_token,
                &watch.watch_channel_id,
                &watch.watch_resource_id,
            )
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
    let refresh_token = connection
        .refresh_token
        .as_deref()
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "no refresh token available"))?;

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

use tracing::warn;
