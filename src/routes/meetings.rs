use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde_json::{Value, json};

use tracing::info;

use crate::{
    models::{
        ApiError, CreateMeetingRequest, CurrentUserResponse, MeetingActionResponse,
        MeetingMutationResponse, MeetingsListQuery, UpdateMeetingRequest,
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
    info!(sub = %jwt.sub, "GET /api/v1/me");
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
    info!(sub = %jwt.sub, url = %payload.meeting_url, "POST /api/v1/meetings");
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

        info!(
            meeting_id = %existing.id,
            bot_name = %bot_name,
            join_at = %timing.recall_join_at,
            "creating Recall bot for meeting"
        );

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

        info!(
            meeting_id = %existing.id,
            recall_bot_id = %created_bot.recall_bot_id,
            status = %created_bot.status,
            "Recall bot created, storing in database"
        );

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

        info!(meeting_id = %existing.id, "Recall bot record stored");
    } else {
        info!(meeting_id = %existing.id, "bot already exists, skipping creation");
    }

    let detail = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &result.meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Invalidate cache
    if let Some(redis) = &state.services.redis {
        redis.invalidate_user_caches(&user.user_id).await;
    }

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

pub async fn update_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
    Json(payload): Json<UpdateMeetingRequest>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "PATCH /api/v1/meetings/{{id}}");
    let user = current_user(&state, &jwt).await?;

    // Verify access
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    state
        .services
        .turso
        .update_meeting_fields(
            &meeting_id,
            payload.title.as_deref(),
            payload.scheduled_start_at.as_deref(),
        )
        .await?;

    let detail = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Invalidate cache
    if let Some(redis) = &state.services.redis {
        redis.invalidate_user_caches(&user.user_id).await;
    }

    Ok(Json(json!({ "meeting": detail })))
}

pub async fn list_meetings(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Query(query): Query<MeetingsListQuery>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, "GET /api/v1/meetings");
    let user = current_user(&state, &jwt).await?;
    let limit = query.limit.unwrap_or(25).min(100);
    let offset = query.offset.unwrap_or(0);

    // Try Redis cache first
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached)) = redis.get_cached_meetings(&user.user_id, limit, offset).await {
            info!(sub = %jwt.sub, "cache hit: meetings list");
            if let Ok(value) = serde_json::from_str::<Value>(&cached) {
                return Ok(Json(value));
            }
        }
    }

    let meetings = state
        .services
        .turso
        .list_meetings_for_user(&user.user_id, limit, offset)
        .await?;

    let response = json!({
        "items": meetings,
        "limit": limit,
        "offset": offset,
    });

    // Write to cache
    if let Some(redis) = &state.services.redis {
        let _ = redis.set_cached_meetings(&user.user_id, limit, offset, &response.to_string()).await;
    }

    Ok(Json(response))
}

pub async fn get_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "GET /api/v1/meetings/{{id}}");
    let user = current_user(&state, &jwt).await?;

    // Try Redis cache for transcript (meeting detail with completed transcription)
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached)) = redis.get_cached_transcript(&meeting_id).await {
            info!(meeting_id = %meeting_id, "cache hit: meeting detail");
            if let Ok(value) = serde_json::from_str::<Value>(&cached) {
                return Ok(Json(value));
            }
        }
    }

    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let response = json!({ "meeting": meeting });

    // Cache if meeting is completed (transcript won't change)
    if meeting.status == "completed" {
        if let Some(redis) = &state.services.redis {
            let _ = redis.set_cached_transcript(&meeting_id, &response.to_string()).await;
        }
    }

    Ok(Json(response))
}

pub async fn get_note(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "GET /api/v1/notes/{{id}}");
    let user = current_user(&state, &jwt).await?;

    // Try Redis cache first
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached)) = redis.get_cached_note(&meeting_id).await {
            info!(meeting_id = %meeting_id, "cache hit: note");
            if let Ok(value) = serde_json::from_str::<Value>(&cached) {
                return Ok(Json(value));
            }
        }
    }

    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let response = json!({ "note": meeting.note });

    // Cache if note exists and is ready
    if meeting.note.is_some() {
        if let Some(redis) = &state.services.redis {
            let _ = redis.set_cached_note(&meeting_id, &response.to_string()).await;
        }
    }

    Ok(Json(response))
}

pub async fn cancel_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<MeetingActionResponse>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "POST /api/v1/meetings/{{id}}/cancel");
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

    // Invalidate cache
    if let Some(redis) = &state.services.redis {
        redis.invalidate_user_caches(&user.user_id).await;
    }

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
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "DELETE /api/v1/meetings/{{id}}");
    let user = current_user(&state, &jwt).await?;
    let deleted = state
        .services
        .turso
        .soft_delete_meeting_for_user(&user.user_id, &meeting_id)
        .await?;

    if deleted {
        // Invalidate cache
        if let Some(redis) = &state.services.redis {
            redis.invalidate_user_caches(&user.user_id).await;
        }
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
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "GET /api/v1/meetings/{{id}}/audio");
    let user = current_user(&state, &jwt).await?;

    // Try Redis cache first
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached_url)) = redis.get_cached_audio_url(&meeting_id).await {
            info!(meeting_id = %meeting_id, "cache hit: audio URL");
            return Ok(Json(json!({ "url": cached_url })).into_response());
        }
    }

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
        .await?;

    info!(
        meeting_id = %meeting_id,
        asset_found = asset.is_some(),
        "audio asset lookup"
    );

    let asset = asset
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting has no stored audio asset"))?;

    info!(
        meeting_id = %meeting_id,
        status = ?asset.status,
        bucket = ?asset.storage_bucket,
        key = ?asset.storage_key,
        "audio asset details"
    );

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

    info!(meeting_id = %meeting_id, key = %asset.storage_key.as_deref().unwrap_or(""), "generating presigned URL");

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

    info!(meeting_id = %meeting_id, "presigned URL generated");

    // Cache the presigned URL
    if let Some(redis) = &state.services.redis {
        let _ = redis.set_cached_audio_url(&meeting_id, &playback_url).await;
    }

    Ok(Json(json!({ "url": playback_url })).into_response())
}
