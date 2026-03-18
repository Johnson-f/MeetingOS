use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::{
    routes::state::SseEvent,
    service::{ServiceRegistry, recall_ai::GroqClient},
};

use crate::service::vector::chunker::TranscriptSegment;

use super::constants::{
    JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_STORE_RECORDING_AUDIO,
    JOB_TRANSCRIBE_RECORDING, JOB_VECTORIZE_TRANSCRIPT,
};

async fn broadcast(services: &ServiceRegistry, event_type: &str, meeting_id: Option<&str>) {
    let _ = services.sse_tx.send(SseEvent {
        event_type: event_type.to_owned(),
        meeting_id: meeting_id.map(str::to_owned),
    });

    // Invalidate the meeting owner's cached data
    if let Some(redis) = &services.redis {
        if let Some(mid) = meeting_id {
            if let Ok(Some(user_id)) = services.turso.get_meeting_owner(mid).await {
                redis.invalidate_user_caches(&user_id).await;
            }
        }
    }
}

pub(super) async fn process_recall_event_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let provider_event_id = payload
        .get("provider_event_id")
        .and_then(Value::as_str)
        .context("missing provider_event_id")?;

    let event = services
        .turso
        .get_provider_event("recall_ai", provider_event_id)
        .await?
        .context("provider event not found")?;

    let event_payload: Value = serde_json::from_str(&event.payload_json)?;
    let event_type = event.event_type.as_str();

    info!(event_type = %event_type, provider_event_id = %provider_event_id, "processing recall event");

    if event_type.starts_with("bot.") {
        info!(event_type = %event_type, "applying bot status change");
        services
            .turso
            .apply_recall_bot_event(event_type, &event_payload)
            .await?;
        broadcast(services, "meeting_updated", None).await;
    }

    if event_type == "recording.done" {
        info!("recording.done received, fetching recording media");
        let meeting_id = services
            .turso
            .apply_recall_recording_event(&event_payload)
            .await?
            .context("recording event did not resolve to a meeting")?;

        let dedupe_key = payload
            .get("provider_event_id")
            .and_then(Value::as_str)
            .map(|value| format!("fetch-recording-{value}"));

        services
            .turso
            .enqueue_job(
                JOB_FETCH_RECORDING_MEDIA,
                dedupe_key.as_deref(),
                &json!({
                    "meeting_id": meeting_id,
                    "provider_event_id": provider_event_id,
                }),
            )
            .await?;
    }

    services
        .turso
        .mark_provider_event_processed("recall_ai", provider_event_id)
        .await?;

    Ok(())
}

pub(super) async fn fetch_recording_media_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let meeting_id = payload
        .get("meeting_id")
        .and_then(Value::as_str)
        .context("missing meeting_id")?;

    let recall = services
        .recall_ai
        .as_ref()
        .context("recall ai is not configured")?;

    let bot = services
        .turso
        .get_latest_recall_bot_for_meeting(meeting_id)
        .await?
        .context("no recall bot found for meeting")?;

    info!(meeting_id = %meeting_id, recall_bot_id = %bot.recall_bot_id, "fetching recording media from Recall");
    let response = recall.retrieve_bot(&bot.recall_bot_id).await?;
    let media = recall.extract_recording_media(&response);

    if media.recording_id.is_none() {
        warn!(meeting_id, "recording metadata not ready yet");
        anyhow::bail!("recording metadata is not ready yet");
    }

    let recording = services
        .turso
        .upsert_recording_for_bot(
            meeting_id,
            &bot.recall_bot_id,
            media.recording_id.as_deref(),
            media.duration_seconds,
            media.started_at.as_deref(),
            media.ended_at.as_deref(),
            "ready",
        )
        .await?;

    if let Some(url) = media.audio_download_url.as_deref() {
        info!(meeting_id = %meeting_id, "audio download URL available, enqueuing store job");
        services
            .turso
            .upsert_recording_asset(
                &recording.id,
                "audio_mixed_mp3",
                "recall_ai",
                media.recording_id.as_deref(),
                Some(url),
                Some("audio/mpeg"),
                "source_available",
            )
            .await?;

        services
            .turso
            .enqueue_job(
                JOB_STORE_RECORDING_AUDIO,
                Some(&format!("store-audio-{}", recording.id)),
                &json!({
                    "meeting_id": meeting_id,
                    "recording_id": recording.id,
                }),
            )
            .await?;
    }

    Ok(())
}

pub(super) async fn store_recording_audio_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let recording_id = payload
        .get("recording_id")
        .and_then(Value::as_str)
        .context("missing recording_id")?;

    let recording = services
        .turso
        .get_recording_with_audio_asset(recording_id)
        .await?
        .context("recording asset not found")?;
    let asset = recording
        .audio_asset
        .as_ref()
        .context("audio asset is not available")?;
    let storage = services
        .storage
        .as_ref()
        .context("storage is not configured");

    if matches!(asset.status.as_deref(), Some("stored"))
        && asset.storage_bucket.as_deref().is_some()
        && asset.storage_key.as_deref().is_some()
    {
        services
            .turso
            .enqueue_job(
                JOB_TRANSCRIBE_RECORDING,
                Some(&format!("transcribe-{}", recording.id)),
                &json!({
                    "meeting_id": recording.meeting_id,
                    "recording_id": recording.id,
                }),
            )
            .await?;
        return Ok(());
    }

    if let Err(error) = storage.as_ref() {
        let _ = services
            .turso
            .set_recording_asset_status(recording_id, "audio_mixed_mp3", "upload_failed")
            .await;
        return Err(anyhow::anyhow!(error.to_string()));
    }

    let source_url = asset
        .source_download_url_last_seen
        .as_deref()
        .context("audio source url is missing")?;
    let mime_type = asset.mime_type.as_deref().unwrap_or("audio/mpeg");
    let storage = storage.unwrap_or_else(|_| unreachable!());

    info!(recording_id = %recording_id, "downloading audio from Recall and uploading to R2");
    services
        .turso
        .set_recording_asset_status(recording_id, "audio_mixed_mp3", "uploading")
        .await?;

    let upload_result = async {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()?;
        let audio_bytes = http.get(source_url).send().await?.bytes().await?;
        info!(recording_id = %recording_id, bytes = audio_bytes.len(), "audio downloaded, uploading to R2");
        let checksum_sha256 = format!("{:x}", Sha256::digest(&audio_bytes));
        let byte_size = audio_bytes.len() as i64;
        let storage_key = crate::service::storage::StorageClient::audio_object_key(
            &recording.meeting_id,
            &recording.id,
        );

        storage
            .upload_audio(&storage_key, audio_bytes.to_vec(), mime_type)
            .await?;

        services
            .turso
            .mark_recording_asset_stored(
                recording_id,
                "audio_mixed_mp3",
                storage.bucket(),
                &storage_key,
                byte_size,
                &checksum_sha256,
                mime_type,
            )
            .await?;

        info!(recording_id = %recording_id, "audio stored in R2, enqueuing transcription job");
        services
            .turso
            .enqueue_job(
                JOB_TRANSCRIBE_RECORDING,
                Some(&format!("transcribe-{}", recording.id)),
                &json!({
                    "meeting_id": recording.meeting_id,
                    "recording_id": recording.id,
                }),
            )
            .await?;

        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(error) = upload_result {
        let _ = services
            .turso
            .set_recording_asset_status(recording_id, "audio_mixed_mp3", "upload_failed")
            .await;
        return Err(error);
    }

    Ok(())
}

pub(super) async fn transcribe_recording_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let groq = GroqClient::new(&services.config).context("groq is not configured")?;
    let storage = services
        .storage
        .as_ref()
        .context("storage is not configured")?;
    let recording_id = payload
        .get("recording_id")
        .and_then(Value::as_str)
        .context("missing recording_id")?;

    let recording = services
        .turso
        .get_recording_with_audio_asset(recording_id)
        .await?
        .context("recording asset not found")?;

    let asset = recording
        .audio_asset
        .as_ref()
        .context("audio asset is not available")?;
    let storage_key = asset
        .storage_key
        .as_deref()
        .context("stored audio key is missing")?;

    if !matches!(asset.status.as_deref(), Some("stored")) || asset.storage_bucket.is_none() {
        anyhow::bail!("audio asset is not stored yet");
    }

    info!(recording_id = %recording_id, "downloading audio from R2 for transcription");
    let audio_bytes = storage.download_audio(storage_key).await?;

    const MIN_AUDIO_BYTES: usize = 10_000;
    if audio_bytes.len() < MIN_AUDIO_BYTES {
        warn!(recording_id = %recording_id, bytes = audio_bytes.len(), "audio file too small to be valid, skipping transcription");
        return Ok(());
    }

    info!(recording_id = %recording_id, bytes = audio_bytes.len(), "sending audio to Groq for transcription");
    let groq_response = groq
        .transcribe(audio_bytes, &services.config.groq.transcription_model)
        .await?;

    info!(recording_id = %recording_id, language = ?groq_response.language, "transcription complete, storing result");
    let transcription = services
        .turso
        .replace_transcription(
            &recording.meeting_id,
            recording_id,
            "groq",
            &services.config.groq.transcription_model,
            groq_response.language.as_deref(),
            &groq_response.text,
            &groq_response.raw_json,
            groq_response.segments,
        )
        .await?;

    broadcast(services, "meeting_updated", Some(&recording.meeting_id)).await;
    info!(meeting_id = %recording.meeting_id, transcription_id = %transcription.id, "enqueuing note generation + vectorization jobs");
    services
        .turso
        .enqueue_job(
            JOB_GENERATE_NOTE,
            Some(&format!("generate-note-{}", transcription.id)),
            &json!({
                "meeting_id": recording.meeting_id,
                "transcription_id": transcription.id,
            }),
        )
        .await?;

    services
        .turso
        .enqueue_job(
            JOB_VECTORIZE_TRANSCRIPT,
            Some(&format!("vectorize-{}", transcription.id)),
            &json!({
                "meeting_id": recording.meeting_id,
                "transcription_id": transcription.id,
            }),
        )
        .await?;

    Ok(())
}

pub(super) async fn generate_note_job(services: &ServiceRegistry, payload: &Value) -> Result<()> {
    let groq = GroqClient::new(&services.config).context("groq is not configured")?;
    let meeting_id = payload
        .get("meeting_id")
        .and_then(Value::as_str)
        .context("missing meeting_id")?;
    let transcription_id = payload
        .get("transcription_id")
        .and_then(Value::as_str)
        .context("missing transcription_id")?;

    let transcription = services
        .turso
        .get_transcription(transcription_id)
        .await?
        .context("transcription not found")?;

    info!(meeting_id = %meeting_id, transcription_id = %transcription_id, "sending transcript to Groq for note generation");
    let note = groq
        .generate_note(
            &services.config.groq.notes_model,
            &transcription.full_text.unwrap_or_default(),
        )
        .await?;

    info!(meeting_id = %meeting_id, "note generated, storing in database");
    services
        .turso
        .replace_note(
            meeting_id,
            transcription_id,
            "groq",
            &services.config.groq.notes_model,
            "v1",
            note,
        )
        .await?;

    broadcast(services, "meeting_updated", Some(meeting_id)).await;
    Ok(())
}

pub(super) async fn vectorize_transcript_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let meeting_id = payload
        .get("meeting_id")
        .and_then(Value::as_str)
        .context("missing meeting_id")?;
    let transcription_id = payload
        .get("transcription_id")
        .and_then(Value::as_str)
        .context("missing transcription_id")?;

    info!(meeting_id = %meeting_id, transcription_id = %transcription_id, "vectorizing transcript");

    let transcription = services
        .turso
        .get_transcription_with_segments(transcription_id)
        .await?
        .context("transcription not found")?;

    let owner_id = services
        .turso
        .get_meeting_owner(meeting_id)
        .await?
        .unwrap_or_default();

    // Fetch actual meeting title for chunk context
    let meeting_title =
        if let Ok(Some(user_id)) = services.turso.get_meeting_owner(meeting_id).await {
            services
                .turso
                .get_meeting_for_user(&user_id, meeting_id)
                .await
                .ok()
                .flatten()
                .map(|m| m.title)
                .unwrap_or_else(|| meeting_id.to_owned())
        } else {
            meeting_id.to_owned()
        };

    let segments: Vec<TranscriptSegment> = transcription
        .segments
        .into_iter()
        .map(|s| TranscriptSegment {
            text: s.text,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            speaker_label: s.speaker_label,
        })
        .collect();

    crate::service::vector::vectorize_transcript(
        services,
        meeting_id,
        &owner_id,
        &meeting_title,
        segments,
        transcription.full_text.as_deref(),
    )
    .await?;

    info!(meeting_id = %meeting_id, "transcript vectorization complete");
    Ok(())
}

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
                warn!(error = %e, "failed to refresh Google token, using existing");
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

pub(super) async fn vectorize_chat_qa_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let thread_id = payload
        .get("thread_id")
        .and_then(Value::as_str)
        .context("missing thread_id")?;
    let user_id = payload
        .get("user_id")
        .and_then(Value::as_str)
        .context("missing user_id")?;
    let question = payload
        .get("question")
        .and_then(Value::as_str)
        .context("missing question")?;
    let answer = payload
        .get("answer")
        .and_then(Value::as_str)
        .context("missing answer")?;

    info!(thread_id = %thread_id, "vectorizing chat Q&A");
    crate::service::vector::vectorize_chat_qa(services, thread_id, user_id, question, answer)
        .await?;
    Ok(())
}

pub(super) async fn schedule_meeting_bots_job(
    services: &ServiceRegistry,
    _payload: &Value,
) -> Result<()> {
    let recall = services
        .recall_ai
        .as_ref()
        .context("Recall AI is not configured")?;

    // Find meetings from calendar that need bots scheduled
    // (status = "draft", source = "google_calendar", scheduled_start_at within 12 min)
    let conn = services.turso.connection().await?;
    let now = chrono::Utc::now();
    let threshold = (now + chrono::Duration::minutes(12)).to_rfc3339();

    info!(now = %now.to_rfc3339(), threshold = %threshold, "checking for draft calendar meetings starting within 12 minutes");

    let mut rows = conn
        .query(
            r#"
            SELECT m.id, m.original_meeting_url, m.scheduled_start_at, m.created_by_user_id, m.workspace_id
            FROM meetings m
            INNER JOIN meeting_access ma ON ma.meeting_id = m.id
            WHERE m.status = 'draft'
              AND m.source = 'google_calendar'
              AND m.scheduled_start_at IS NOT NULL
              AND m.scheduled_start_at <= ?
              AND m.deleted_at IS NULL
            GROUP BY m.id
            "#,
            libsql::params![threshold.as_str()],
        )
        .await?;

    let mut scheduled_count = 0;
    while let Some(row) = rows.next().await? {
        let meeting_id = row.get::<String>(0)?;
        let meeting_url = row.get::<String>(1)?;
        let scheduled_start = row.get::<Option<String>>(2)?;
        let user_id = row.get::<String>(3)?;
        let workspace_id = row.get::<String>(4)?;

        // Calculate join_at (10 min before start)
        let join_at = if let Some(ref start) = scheduled_start {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(start) {
                let join = dt - chrono::Duration::minutes(10);
                if join.with_timezone(&chrono::Utc) < now {
                    now.to_rfc3339()
                } else {
                    join.to_rfc3339()
                }
            } else {
                now.to_rfc3339()
            }
        } else {
            now.to_rfc3339()
        };

        let bot_name = recall.default_bot_name();
        info!(meeting_id = %meeting_id, scheduled_start = ?scheduled_start, join_at = %join_at, meeting_url = %meeting_url, "dispatching bot for calendar meeting");

        match recall
            .create_bot(crate::service::recall_ai::RecallCreateBotRequest {
                meeting_url: &meeting_url,
                bot_name,
                join_at: &join_at,
                metadata: serde_json::json!({
                    "meeting_id": meeting_id,
                    "workspace_id": workspace_id,
                    "user_id": user_id,
                }),
            })
            .await
        {
            Ok(created_bot) => {
                services
                    .turso
                    .store_recall_bot(
                        &meeting_id,
                        &created_bot.recall_bot_id,
                        bot_name,
                        &join_at,
                        &created_bot.status,
                        &created_bot.raw_json.to_string(),
                    )
                    .await?;
                scheduled_count += 1;
            }
            Err(e) => {
                warn!(meeting_id = %meeting_id, error = %e, "failed to schedule bot for calendar meeting");
            }
        }
    }

    if scheduled_count > 0 {
        info!(
            count = scheduled_count,
            "scheduled bots for calendar meetings"
        );
    } else {
        info!("no draft calendar meetings need bots right now");
    }
    Ok(())
}

pub(super) async fn migrate_chat_vectors_job(
    services: &ServiceRegistry,
    _payload: &Value,
) -> Result<()> {
    let qdrant_transcripts = services.qdrant.as_ref().context("qdrant not configured")?;
    let qdrant_chat = services.qdrant_chat.as_ref().context("qdrant_chat not configured")?;

    info!("migrating chat vectors from transcript collection to chat collection");

    let chat_points = qdrant_transcripts.extract_chat_points().await?;
    if chat_points.is_empty() {
        info!("no chat points to migrate");
        return Ok(());
    }

    info!(count = chat_points.len(), "extracted chat points, upserting to chat collection");
    qdrant_chat.upsert_chat_qa_points(chat_points).await?;

    qdrant_transcripts.delete_chat_points().await?;
    info!("chat vector migration complete");
    Ok(())
}
