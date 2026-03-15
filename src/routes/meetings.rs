use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde_json::{Value, json};

use crate::{
    models::{
        ApiError, CreateMeetingRequest, CurrentUserResponse, MeetingActionResponse,
        MeetingMutationResponse, MeetingsListQuery,
    },
    service::{
        recall_ai::{
            RecallCreateBotRequest, build_dedup_key, normalize_meeting_url, platform_from_url,
            resolve_meeting_timing,
        },
        turso::read_operations::MeetingDraft,
    },
};

use super::{
    helpers::{current_user, recall_unavailable_error},
    state::AppState,
};

pub async fn me(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<Json<CurrentUserResponse>, ApiError> {
    let user = current_user(&state, &jwt).await?;
    Ok(Json(CurrentUserResponse {
        user_id: user.user_id,
        workspace_id: user.workspace_id,
    }))
}

pub async fn create_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Json(payload): Json<CreateMeetingRequest>,
) -> Result<(StatusCode, Json<MeetingMutationResponse>), ApiError> {
    let user = current_user(&state, &jwt).await?;
    let recall = state
        .services
        .recall_ai
        .as_ref()
        .ok_or_else(recall_unavailable_error)?;

    let normalized_url = normalize_meeting_url(&payload.meeting_url);
    let title = payload
        .title
        .clone()
        .unwrap_or_else(|| format!("{} meeting", platform_from_url(&payload.meeting_url)));
    let timing = resolve_meeting_timing(
        payload.meeting_time_mode.as_str(),
        payload.join_at.as_deref(),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "invalid meeting timing for mode {}: {error}",
                payload.meeting_time_mode.as_str()
            ),
        )
    })?;
    let dedup_key = build_dedup_key(&user.workspace_id, &normalized_url, &timing.dedup_time_key);

    let result = state
        .services
        .turso
        .create_or_get_meeting(
            &user,
            &MeetingDraft {
                title,
                source: "manual".to_owned(),
                original_meeting_url: payload.meeting_url.clone(),
                normalized_meeting_url: normalized_url,
                platform: platform_from_url(&payload.meeting_url),
                meeting_time_mode: payload.meeting_time_mode.as_str().to_owned(),
                dedup_key,
                scheduled_start_at: timing.scheduled_start_at.clone(),
            },
        )
        .await?;

    let existing = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &result.meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found after creation"))?;

    if existing.bot.is_none() {
        let bot_name = payload
            .bot_name
            .as_deref()
            .unwrap_or_else(|| recall.default_bot_name());

        let created_bot = recall
            .create_bot(RecallCreateBotRequest {
                meeting_url: &payload.meeting_url,
                bot_name,
                join_at: &timing.recall_join_at,
                metadata: json!({
                    "meeting_id": existing.id,
                    "workspace_id": user.workspace_id,
                    "user_id": user.user_id,
                }),
            })
            .await
            .map_err(|error| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to create Recall bot: {error}"),
                )
            })?;

        state
            .services
            .turso
            .store_recall_bot(
                &existing.id,
                &created_bot.recall_bot_id,
                bot_name,
                &timing.recall_join_at,
                &created_bot.status,
                &created_bot.raw_json.to_string(),
            )
            .await?;
    }

    let detail = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &result.meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    Ok((
        if result.created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(MeetingMutationResponse {
            meeting: detail,
            created: result.created,
        }),
    ))
}

pub async fn list_meetings(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Query(query): Query<MeetingsListQuery>,
) -> Result<Json<Value>, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let limit = query.limit.unwrap_or(25).min(100);
    let offset = query.offset.unwrap_or(0);
    let meetings = state
        .services
        .turso
        .list_meetings_for_user(&user.user_id, limit, offset)
        .await?;

    Ok(Json(json!({
        "items": meetings,
        "limit": limit,
        "offset": offset,
    })))
}

pub async fn get_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;
    Ok(Json(json!({ "meeting": meeting })))
}

pub async fn get_note(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;
    Ok(Json(json!({ "note": meeting.note })))
}

pub async fn cancel_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<MeetingActionResponse>, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let recall = state
        .services
        .recall_ai
        .as_ref()
        .ok_or_else(recall_unavailable_error)?;

    let bot = meeting
        .bot
        .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "meeting has no active bot"))?;

    if matches!(bot.status.as_str(), "scheduled" | "requested") {
        recall
            .cancel_scheduled_bot(&bot.recall_bot_id)
            .await
            .map_err(|error| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to cancel scheduled bot: {error}"),
                )
            })?;
    } else {
        recall
            .leave_call(&bot.recall_bot_id)
            .await
            .map_err(|error| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to remove bot from call: {error}"),
                )
            })?;
    }

    state
        .services
        .turso
        .update_meeting_status(&meeting_id, "cancelled", Some("cancelled"))
        .await?;

    Ok(Json(MeetingActionResponse {
        meeting_id,
        status: "cancelled".to_owned(),
        processing_status: "cancelled".to_owned(),
    }))
}

pub async fn delete_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let deleted = state
        .services
        .turso
        .soft_delete_meeting_for_user(&user.user_id, &meeting_id)
        .await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))
    }
}

pub async fn get_audio(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Response, ApiError> {
    let user = current_user(&state, &jwt).await?;
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let asset = state
        .services
        .turso
        .get_audio_asset_for_meeting(&meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting has no stored audio asset"))?;

    if !matches!(asset.status.as_deref(), Some("stored"))
        || asset.storage_bucket.is_none()
        || asset.storage_key.is_none()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "audio is not ready for playback yet",
        ));
    }

    let storage = state.services.storage.as_ref().ok_or_else(|| {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "storage is not configured")
    })?;
    let playback_url = storage
        .presign_audio_get(
            asset.storage_key.as_deref().unwrap_or_default(),
            std::time::Duration::from_secs(300),
        )
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to create playback url: {error}"),
            )
        })?;

    Ok((
        StatusCode::FOUND,
        [
            (header::LOCATION, playback_url),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
    )
        .into_response())
}
