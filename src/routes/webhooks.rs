use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

use tracing::{info, warn};

use crate::models::{ApiError, RecallWebhookAck};

use super::{
    helpers::{extract_subject_id, header_string, recall_unavailable_error},
    state::AppState,
};

pub async fn recall_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<RecallWebhookAck>, ApiError> {
    let recall = state
        .services
        .recall_ai
        .as_ref()
        .ok_or_else(recall_unavailable_error)?;

    let raw_body = std::str::from_utf8(&body)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid webhook body encoding"))?;
    let message_id = header_string(&headers, &["webhook-id", "svix-id"])
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing webhook id"))?;
    let timestamp = header_string(&headers, &["webhook-timestamp", "svix-timestamp"])
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing webhook timestamp"))?;
    let signature = header_string(&headers, &["webhook-signature", "svix-signature"])
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing webhook signature"))?;
    let signature_verified = recall
        .verify_webhook(&message_id, &timestamp, &signature, raw_body)
        .map_err(|error| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                format!("webhook signature verification failed: {error}"),
            )
        })?;

    if !signature_verified {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "webhook signature is invalid",
        ));
    }

    let payload: Value = serde_json::from_str(raw_body)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid webhook json"))?;
    let provider_event_id = header_string(&headers, &["webhook-id", "svix-id"])
        .or_else(|| payload.get("id").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let event_type = payload
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let subject_id = extract_subject_id(&payload);

    info!(
        event_type = %event_type,
        subject_id = ?subject_id,
        "webhook received from Recall"
    );

    let inserted = state
        .services
        .turso
        .store_provider_event(
            "recall_ai",
            &provider_event_id,
            &event_type,
            subject_id.as_deref(),
            raw_body,
            signature_verified,
        )
        .await?;

    if inserted {
        info!(event_type = %event_type, "event stored, enqueuing job");
        state
            .services
            .turso
            .enqueue_job(
                "process_recall_event",
                Some(&provider_event_id),
                &json!({ "provider_event_id": provider_event_id }),
            )
            .await?;
    }

    Ok(Json(RecallWebhookAck {
        accepted: true,
        provider_event_id,
        event_type,
    }))
}

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
