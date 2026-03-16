use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::service::{ServiceRegistry, recall_ai::GroqClient};

use super::constants::{
    JOB_FETCH_RECORDING_MEDIA, JOB_GENERATE_NOTE, JOB_STORE_RECORDING_AUDIO,
    JOB_TRANSCRIBE_RECORDING,
};

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
        let audio_bytes = reqwest::get(source_url).await?.bytes().await?;
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

    info!(meeting_id = %recording.meeting_id, transcription_id = %transcription.id, "enqueuing note generation job");
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

    Ok(())
}
