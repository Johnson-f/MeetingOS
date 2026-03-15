use anyhow::{Context, Result};
use serde_json::{Value, json};
use tracing::warn;

use crate::service::{ServiceRegistry, recall_ai::GroqClient};

use super::constants::{JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_TRANSCRIBE_RECORDING};

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

    if event_type.starts_with("bot.") {
        services
            .turso
            .apply_recall_bot_event(event_type, &event_payload)
            .await?;
    }

    if event_type == "recording.done" {
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

    let response = recall.retrieve_bot(&bot.recall_bot_id).await?;
    let media = recall.extract_recording_media(&response);

    if media.recording_id.is_none() {
        warn!(meeting_id, "recording metadata not ready yet");
        return Ok(());
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
                JOB_TRANSCRIBE_RECORDING,
                Some(&format!("transcribe-{}", recording.id)),
                &json!({
                    "meeting_id": meeting_id,
                    "recording_id": recording.id,
                }),
            )
            .await?;
    }

    Ok(())
}

pub(super) async fn transcribe_recording_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let groq = GroqClient::new(&services.config).context("groq is not configured")?;
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
    let source_url = asset
        .source_download_url_last_seen
        .as_deref()
        .context("audio source url is missing")?;

    let audio_bytes = reqwest::get(source_url).await?.bytes().await?;
    let groq_response = groq
        .transcribe(
            audio_bytes.to_vec(),
            &services.config.groq.transcription_model,
        )
        .await?;

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

    let note = groq
        .generate_note(
            &services.config.groq.notes_model,
            &transcription.full_text.unwrap_or_default(),
        )
        .await?;

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

    Ok(())
}
