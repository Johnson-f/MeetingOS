use anyhow::{Context, Result};
use comrak::{Options, markdown_to_html};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::{
    routes::state::SseEvent,
    service::{ServiceRegistry, recall_ai::GroqClient},
};

use crate::service::vector::chunker::TranscriptSegment;

use super::constants::{
    JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_SEND_SHARE_EMAILS, JOB_STORE_RECORDING_AUDIO,
    JOB_TRANSCRIBE_RECORDING, JOB_VECTORIZE_TRANSCRIPT,
};

async fn broadcast(services: &ServiceRegistry, event_type: &str, meeting_id: Option<&str>) {
    broadcast_with_data(services, event_type, meeting_id, None).await;
}

async fn broadcast_with_data(
    services: &ServiceRegistry,
    event_type: &str,
    meeting_id: Option<&str>,
    data: Option<serde_json::Value>,
) {
    let _ = services.sse_tx.send(SseEvent {
        event_type: event_type.to_owned(),
        meeting_id: meeting_id.map(str::to_owned),
        data,
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

    // Extract participants from bot response
    let participants = recall.extract_participants(&response);
    for p in &participants {
        let participant_id = services
            .turso
            .upsert_recall_participant(
                meeting_id,
                media.recording_id.as_deref(),
                &p.id,
                &p.name,
                p.is_host,
            )
            .await?;

        for event in &p.events {
            services
                .turso
                .insert_participant_event(
                    meeting_id,
                    media.recording_id.as_deref(),
                    &participant_id,
                    &event.event_type,
                    event.timestamp.as_deref(),
                    event.relative_ms,
                    "{}",
                )
                .await?;

            if let Some(ts) = event.timestamp.as_deref() {
                match event.event_type.as_str() {
                    "join" => {
                        services
                            .turso
                            .update_participant_timestamps(&participant_id, ts, "")
                            .await?;
                    }
                    "leave" => {
                        services
                            .turso
                            .update_participant_timestamps(&participant_id, "", ts)
                            .await?;
                    }
                    _ => {}
                }
            }
        }
    }
    info!(meeting_id = %meeting_id, participant_count = participants.len(), "extracted participants from Recall");

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

    // Store speaker diarization timeline on the recording
    let speaker_timeline = recall.extract_speaker_timeline(&response);
    if !speaker_timeline.is_empty() {
        if let Ok(timeline_json) = serde_json::to_string(&speaker_timeline) {
            let _ = services
                .turso
                .store_speaker_timeline(&recording.id, &timeline_json)
                .await;
            info!(
                meeting_id = %meeting_id,
                recording_id = %recording.id,
                spans = speaker_timeline.len(),
                "stored speaker diarization timeline"
            );
        }
    }

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

    // Re-label speaker labels using diarization timeline if available
    let mut segments = groq_response.segments;
    if let Ok(Some(timeline_json)) = services.turso.get_speaker_timeline(recording_id).await {
        if let Ok(timeline) = serde_json::from_str::<Vec<crate::service::recall_ai::SpeakerSpan>>(&timeline_json) {
            if !timeline.is_empty() {
                segments = relabel_segments_with_speakers(&segments, &timeline);
                info!(
                    recording_id = %recording_id,
                    "re-labeled transcript segments with speaker names from diarization"
                );
            }
        }
    }

    // Build speaker-attributed transcript for note generation
    // This replaces the raw Whisper full_text with "Speaker: text" format
    let full_text = build_speaker_attributed_text(&segments, &groq_response.text);

    info!(recording_id = %recording_id, language = ?groq_response.language, "transcription complete, storing result");
    let transcription = services
        .turso
        .replace_transcription(
            &recording.meeting_id,
            recording_id,
            "groq",
            &services.config.groq.transcription_model,
            groq_response.language.as_deref(),
            &full_text,
            &groq_response.raw_json,
            segments,
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

    // Chain auto-share: if enabled, add participants as share recipients and enqueue email job
    if services
        .turso
        .is_auto_share_enabled(meeting_id)
        .await
        .unwrap_or(false)
    {
        let participants = services
            .turso
            .get_participants_with_emails(meeting_id)
            .await
            .unwrap_or_default();
        if !participants.is_empty() {
            let owner_id = services
                .turso
                .get_meeting_owner(meeting_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            let recipients: Vec<(String, Option<String>, Option<String>)> = participants
                .iter()
                .map(|(id, email, name)| (email.clone(), name.clone(), Some(id.clone())))
                .collect();
            if let Err(e) = services
                .turso
                .add_share_recipients(meeting_id, recipients, "auto")
                .await
            {
                warn!(meeting_id = %meeting_id, error = %e, "failed to add auto share recipients");
            }

            let expiry_days = services.config.resend.share_token_expiry_days;
            match services
                .turso
                .get_or_create_share_token(meeting_id, &owner_id, expiry_days)
                .await
            {
                Ok(token) => {
                    if let Err(e) = services
                        .turso
                        .enqueue_job(
                            JOB_SEND_SHARE_EMAILS,
                            Some(&format!("send-share-{meeting_id}")),
                            &json!({
                                "meeting_id": meeting_id,
                                "share_token": token,
                            }),
                        )
                        .await
                    {
                        warn!(meeting_id = %meeting_id, error = %e, "failed to enqueue send_share_emails job");
                    } else {
                        info!(meeting_id = %meeting_id, "enqueued send_share_emails job for auto-share");
                    }
                }
                Err(e) => {
                    warn!(meeting_id = %meeting_id, error = %e, "failed to create share token for auto-share");
                }
            }
        }
    }

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
    let threshold = (now + chrono::Duration::minutes(5)).to_rfc3339();

    info!(now = %now.to_rfc3339(), threshold = %threshold, "checking for draft calendar meetings starting within 5 minutes");

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

        // Calculate join_at (3 min before start)
        let join_at = if let Some(ref start) = scheduled_start {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(start) {
                let join = dt - chrono::Duration::minutes(3);
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

                // Copy calendar attendees into participants
                if let Ok(attendees) = services
                    .turso
                    .get_calendar_attendees_for_meeting(&meeting_id)
                    .await
                {
                    for attendee in &attendees {
                        let _ = services
                            .turso
                            .upsert_calendar_participant(
                                &meeting_id,
                                &attendee.email,
                                attendee.display_name.as_deref(),
                                attendee.is_organizer,
                            )
                            .await;
                    }
                    if !attendees.is_empty() {
                        info!(meeting_id = %meeting_id, count = attendees.len(), "copied calendar attendees to participants");
                    }
                }

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
    let qdrant_chat = services
        .qdrant_chat
        .as_ref()
        .context("qdrant_chat not configured")?;

    info!("migrating chat vectors from transcript collection to chat collection");

    let chat_points = qdrant_transcripts.extract_chat_points().await?;
    if chat_points.is_empty() {
        info!("no chat points to migrate");
        return Ok(());
    }

    info!(
        count = chat_points.len(),
        "extracted chat points, upserting to chat collection"
    );
    qdrant_chat.upsert_chat_qa_points(chat_points).await?;

    qdrant_transcripts.delete_chat_points().await?;
    info!("chat vector migration complete");
    Ok(())
}

pub(super) async fn send_share_emails_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let meeting_id = payload
        .get("meeting_id")
        .and_then(Value::as_str)
        .context("missing meeting_id")?;
    let share_token = payload
        .get("share_token")
        .and_then(Value::as_str)
        .context("missing share_token")?;

    let resend = services
        .resend
        .as_ref()
        .context("resend is not configured")?;

    let owner_id = services
        .turso
        .get_meeting_owner(meeting_id)
        .await?
        .context("meeting owner not found")?;

    let meeting = services
        .turso
        .get_meeting_for_user(&owner_id, meeting_id)
        .await?
        .context("meeting not found")?;

    let note = match &meeting.note {
        Some(n) if n.status == "ready" => n.clone(),
        Some(_) => {
            // Note not ready yet — bail so the job retries
            anyhow::bail!("note is not ready yet, will retry");
        }
        None => {
            anyhow::bail!("no note available yet, will retry");
        }
    };

    let frontend_url = &services.config.frontend_url;
    let share_link = format!(
        "{}/share/{}",
        frontend_url.trim_end_matches('/'),
        share_token
    );

    let note_title = note.title.as_deref().unwrap_or(&meeting.title);
    let summary_html = note
        .summary_markdown
        .as_deref()
        .map(|md| markdown_to_html(md, &Options::default()))
        .unwrap_or_default();

    let key_points_html = if note.key_points.is_empty() {
        String::new()
    } else {
        let items: String = note
            .key_points
            .iter()
            .map(|p| format!("<li>{}</li>", html_escape(p)))
            .collect();
        format!(
            "<h3 style=\"margin:24px 0 8px;\">Key Points</h3><ul style=\"margin:0;padding-left:20px;\">{items}</ul>"
        )
    };

    let action_items_html = if note.action_items.is_empty() {
        String::new()
    } else {
        let items: String = note
            .action_items
            .iter()
            .map(|a| {
                let assignee = a
                    .assignee_name
                    .as_deref()
                    .map(|n| format!(" — <em>{}</em>", html_escape(n)))
                    .unwrap_or_default();
                format!("<li>{}{}</li>", html_escape(&a.description), assignee)
            })
            .collect();
        format!(
            "<h3 style=\"margin:24px 0 8px;\">Action Items</h3><ul style=\"margin:0;padding-left:20px;\">{items}</ul>"
        )
    };

    let html_body = format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>{title}</title></head>
<body style="font-family:sans-serif;max-width:600px;margin:0 auto;padding:24px;color:#222;">
  <h2 style="margin-top:0;">{title}</h2>
  <div style="margin-bottom:16px;">{summary}</div>
  {key_points}
  {action_items}
  <p style="margin-top:32px;">
    <a href="{link}" style="background:#2563eb;color:#fff;padding:10px 20px;border-radius:6px;text-decoration:none;font-weight:bold;">View Full Meeting Notes</a>
  </p>
  <p style="font-size:12px;color:#888;margin-top:24px;">This meeting summary was shared with you. <a href="{link}">{link}</a></p>
</body>
</html>"#,
        title = html_escape(note_title),
        summary = summary_html,
        key_points = key_points_html,
        action_items = action_items_html,
        link = html_escape(&share_link),
    );

    let subject = format!("Meeting notes: {}", note_title);

    let recipients = services
        .turso
        .get_pending_share_recipients(meeting_id)
        .await?;

    info!(
        meeting_id = %meeting_id,
        recipient_count = recipients.len(),
        "sending share emails"
    );

    let mut sent_count = 0u32;
    let mut failed_count = 0u32;
    let mut failed_emails: Vec<(String, String)> = Vec::new(); // (email, reason)

    for recipient in &recipients {
        // MX validation: check domain has mail servers
        let domain = recipient.email.split('@').nth(1).unwrap_or("");
        if domain.is_empty() {
            let reason = "invalid email: missing domain".to_owned();
            warn!(meeting_id = %meeting_id, email = %recipient.email, "skipping: {}", reason);
            services
                .turso
                .record_email_delivery(
                    meeting_id,
                    &recipient.email,
                    "validation",
                    "failed",
                    None,
                    Some(&reason),
                )
                .await?;
            failed_count += 1;
            failed_emails.push((recipient.email.clone(), reason));
            continue;
        }

        match validate_mx(domain).await {
            Ok(false) => {
                let reason = format!("no mail server found for domain '{}'", domain);
                warn!(meeting_id = %meeting_id, email = %recipient.email, "skipping: {}", reason);
                services
                    .turso
                    .record_email_delivery(
                        meeting_id,
                        &recipient.email,
                        "validation",
                        "failed",
                        None,
                        Some(&reason),
                    )
                    .await?;
                failed_count += 1;
                failed_emails.push((recipient.email.clone(), reason));
                continue;
            }
            Err(e) => {
                // DNS lookup failed — don't block sending, just warn
                warn!(meeting_id = %meeting_id, email = %recipient.email, error = %e, "MX lookup failed, sending anyway");
            }
            Ok(true) => {}
        }

        match resend
            .send_email(&recipient.email, &subject, &html_body)
            .await
        {
            Ok(result) => {
                services
                    .turso
                    .record_email_delivery(
                        meeting_id,
                        &recipient.email,
                        "resend",
                        "sent",
                        result.provider_message_id.as_deref(),
                        None,
                    )
                    .await?;
                sent_count += 1;
                info!(meeting_id = %meeting_id, email = %recipient.email, "share email sent");
            }
            Err(e) => {
                let error_msg = e.to_string();
                warn!(meeting_id = %meeting_id, email = %recipient.email, error = %error_msg, "failed to send share email");
                services
                    .turso
                    .record_email_delivery(
                        meeting_id,
                        &recipient.email,
                        "resend",
                        "failed",
                        None,
                        Some(&error_msg),
                    )
                    .await?;
                failed_count += 1;
                failed_emails.push((recipient.email.clone(), error_msg));
            }
        }
    }

    // Broadcast result to frontend via SSE
    let status = if failed_count == 0 {
        "success"
    } else if sent_count == 0 {
        "failed"
    } else {
        "partial"
    };
    broadcast_with_data(
        services,
        "share_status",
        Some(meeting_id),
        Some(json!({
            "status": status,
            "sent_count": sent_count,
            "failed_count": failed_count,
            "failed_emails": failed_emails.iter().map(|(email, reason)| json!({ "email": email, "reason": reason })).collect::<Vec<_>>(),
        })),
    )
    .await;

    Ok(())
}

async fn validate_mx(domain: &str) -> Result<bool> {
    use trust_dns_resolver::TokioAsyncResolver;
    let resolver =
        TokioAsyncResolver::tokio_from_system_conf().context("failed to create DNS resolver")?;
    let mx_lookup = resolver.mx_lookup(domain).await;
    match mx_lookup {
        Ok(records) => Ok(records.iter().next().is_some()),
        Err(e) => {
            // NXDOMAIN or NODATA means no MX records → domain doesn't accept mail
            let kind = e.kind();
            if matches!(
                kind,
                trust_dns_resolver::error::ResolveErrorKind::NoRecordsFound { .. }
            ) {
                Ok(false)
            } else {
                Err(anyhow::anyhow!("DNS lookup error for {}: {}", domain, e))
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// For each transcript segment, find the speaker span with the most temporal overlap
/// and replace the segment's speaker_label with the speaker's actual name.
fn relabel_segments_with_speakers(
    segments: &[crate::service::recall_ai::GroqSegment],
    timeline: &[crate::service::recall_ai::SpeakerSpan],
) -> Vec<crate::service::recall_ai::GroqSegment> {
    segments
        .iter()
        .map(|seg| {
            let mut best_name: Option<&str> = None;
            let mut best_overlap: i64 = 0;

            for span in timeline {
                let overlap_start = seg.start_ms.max(span.start_ms);
                let overlap_end = seg.end_ms.min(span.end_ms);
                let overlap = (overlap_end - overlap_start).max(0);

                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_name = Some(&span.name);
                }
            }

            let mut relabeled = seg.clone();
            if let Some(name) = best_name {
                relabeled.speaker_label = Some(name.to_owned());
            }
            relabeled
        })
        .collect()
}

/// Build a speaker-attributed transcript from re-labeled segments.
/// Groups consecutive segments by the same speaker to avoid repetitive labels.
/// Falls back to the raw Whisper text if no segments have speaker labels.
fn build_speaker_attributed_text(
    segments: &[crate::service::recall_ai::GroqSegment],
    fallback_text: &str,
) -> String {
    let has_speakers = segments.iter().any(|s| s.speaker_label.is_some());
    if !has_speakers {
        return fallback_text.to_owned();
    }

    let mut lines = Vec::new();
    let mut current_speaker: Option<&str> = None;
    let mut current_text = String::new();

    for seg in segments {
        let speaker = seg.speaker_label.as_deref().unwrap_or("Unknown");

        if current_speaker == Some(speaker) {
            // Same speaker — append text
            current_text.push(' ');
            current_text.push_str(seg.text.trim());
        } else {
            // New speaker — flush previous
            if !current_text.is_empty() {
                lines.push(format!(
                    "{}: {}",
                    current_speaker.unwrap_or("Unknown"),
                    current_text.trim()
                ));
            }
            current_speaker = Some(speaker);
            current_text = seg.text.trim().to_owned();
        }
    }

    // Flush last speaker
    if !current_text.is_empty() {
        lines.push(format!(
            "{}: {}",
            current_speaker.unwrap_or("Unknown"),
            current_text.trim()
        ));
    }

    lines.join("\n")
}
