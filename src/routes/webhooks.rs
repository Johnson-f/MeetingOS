use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

use tracing::info;

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
