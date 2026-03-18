use axum::extract::Extension;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::{models::ApiError, service::jobs::constants::JOB_SEND_SHARE_EMAILS};

use super::{helpers::current_user, state::AppState};

#[derive(Debug, Deserialize)]
pub struct ShareRequest {
    pub emails: Vec<String>,
}

pub async fn share_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
    Json(payload): Json<ShareRequest>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "POST /api/v1/meetings/{{meeting_id}}/share");
    let user = current_user(&state, &jwt).await?;

    // Verify meeting ownership
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Validate emails
    if payload.emails.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "emails must not be empty",
        ));
    }
    if payload.emails.len() > 50 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "too many recipients; maximum is 50",
        ));
    }
    for email in &payload.emails {
        if !email.contains('@') || !email.contains('.') {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid email address: {email}"),
            ));
        }
    }

    // Check resend is configured
    if state.services.resend.is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email service is not configured",
        ));
    }

    let recipient_count = payload.emails.len();

    // Add recipients with source "manual"
    let recipients: Vec<(String, Option<String>, Option<String>)> = payload
        .emails
        .into_iter()
        .map(|email| (email, None, None))
        .collect();
    state
        .services
        .turso
        .add_share_recipients(&meeting_id, recipients, "manual")
        .await?;

    // Get or create share token
    let expiry_days = state.services.config.resend.share_token_expiry_days;
    let token = state
        .services
        .turso
        .get_or_create_share_token(&meeting_id, &user.user_id, expiry_days)
        .await?;

    // Enqueue send_share_emails job
    state
        .services
        .turso
        .enqueue_job(
            JOB_SEND_SHARE_EMAILS,
            Some(meeting_id.as_str()),
            &json!({ "meeting_id": meeting_id, "share_token": token }),
        )
        .await?;

    info!(
        meeting_id = %meeting_id,
        recipient_count = recipient_count,
        "share email job enqueued"
    );

    Ok(Json(json!({
        "status": "queued",
        "recipient_count": recipient_count
    })))
}

pub async fn list_participants(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "GET /api/v1/meetings/{{meeting_id}}/participants");
    let user = current_user(&state, &jwt).await?;

    // Verify access
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let participants = state
        .services
        .turso
        .list_participants_for_meeting(&meeting_id)
        .await?;

    Ok(Json(json!({ "participants": participants })))
}

pub async fn get_shared_meeting(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Value>, ApiError> {
    info!(token = %token, "GET /api/v1/share/{{token}}");

    // Validate token
    let meeting_id = state
        .services
        .turso
        .validate_share_token(&token)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "invalid or expired share token"))?;

    // Get meeting owner
    let owner_id = state
        .services
        .turso
        .get_meeting_owner(&meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Get meeting detail via owner lookup
    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&owner_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Get participants
    let participants = state
        .services
        .turso
        .list_participants_for_meeting(&meeting_id)
        .await?;

    // Generate presigned audio URL if available
    let audio_url: Option<String> = if let Some(storage) = state.services.storage.as_ref() {
        let asset = state
            .services
            .turso
            .get_audio_asset_for_meeting(&meeting_id)
            .await?;

        if let Some(asset) = asset {
            if matches!(asset.status.as_deref(), Some("stored")) && asset.storage_key.is_some() {
                match storage
                    .presign_audio_get(
                        asset.storage_key.as_deref().unwrap_or_default(),
                        std::time::Duration::from_secs(300),
                    )
                    .await
                {
                    Ok(url) => Some(url),
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(json!({
        "meeting": {
            "id": meeting.id,
            "title": meeting.title,
            "platform": meeting.platform,
            "status": meeting.status,
            "scheduled_start_at": meeting.scheduled_start_at,
            "actual_start_at": meeting.actual_start_at,
            "actual_end_at": meeting.actual_end_at,
            "note": meeting.note,
            "transcription": meeting.transcription,
        },
        "participants": participants,
        "audio_url": audio_url,
    })))
}
