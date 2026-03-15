use anyhow::Result;
use libsql::params;

use crate::service::{
    recall_ai::{GeneratedNote, GroqSegment},
    turso::{
        client::{new_id, now_rfc3339},
        read_operations::{
            RecordingRow, StoredRecordingAudioAsset, StoredRecordingWithAsset, StoredTranscription,
            helpers::query_optional_string,
        },
    },
};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn upsert_recording_for_bot(
        &self,
        meeting_id: &str,
        recall_bot_id: &str,
        recall_recording_id: Option<&str>,
        duration_seconds: Option<i64>,
        started_at: Option<&str>,
        ended_at: Option<&str>,
        status: &str,
    ) -> Result<RecordingRow> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        if let Some(existing_id) = query_optional_string(
            &conn,
            "SELECT id FROM recordings WHERE meeting_id = ? AND ((recall_recording_id IS NOT NULL AND recall_recording_id = ?) OR (recall_bot_id = ?)) LIMIT 1",
            (meeting_id, recall_recording_id, recall_bot_id),
        )
        .await?
        {
            conn.execute(
                "UPDATE recordings
                 SET recall_recording_id = COALESCE(?, recall_recording_id),
                     status = ?, duration_seconds = COALESCE(?, duration_seconds),
                     started_at = COALESCE(?, started_at), ended_at = COALESCE(?, ended_at),
                     updated_at = ?
                 WHERE id = ?",
                (
                    recall_recording_id,
                    status,
                    duration_seconds,
                    started_at,
                    ended_at,
                    now.as_str(),
                    existing_id.as_str(),
                ),
            )
            .await?;
            return Ok(RecordingRow {
                id: existing_id,
                meeting_id: meeting_id.to_owned(),
            });
        }

        let id = new_id();
        conn.execute(
            "INSERT INTO recordings (
                id, meeting_id, recall_bot_id, recall_recording_id, status, duration_seconds, started_at, ended_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                recall_bot_id,
                recall_recording_id,
                status,
                duration_seconds,
                started_at,
                ended_at,
                now.as_str(),
                now.as_str(),
            ),
        )
        .await?;

        Ok(RecordingRow {
            id,
            meeting_id: meeting_id.to_owned(),
        })
    }

    pub async fn upsert_recording_asset(
        &self,
        recording_id: &str,
        asset_kind: &str,
        provider: &str,
        provider_asset_id: Option<&str>,
        source_download_url_last_seen: Option<&str>,
        mime_type: Option<&str>,
        status: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO recording_assets (
                    id, recording_id, asset_kind, provider, provider_asset_id, source_download_url_last_seen, mime_type, status, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    new_id(),
                    recording_id,
                    asset_kind,
                    provider,
                    provider_asset_id,
                    source_download_url_last_seen,
                    mime_type,
                    status,
                    now.as_str(),
                    now.as_str(),
                ),
            )
            .await?;

        if inserted == 0 {
            conn.execute(
                "UPDATE recording_assets
                 SET provider_asset_id = COALESCE(?, provider_asset_id),
                     source_download_url_last_seen = COALESCE(?, source_download_url_last_seen),
                     mime_type = COALESCE(?, mime_type),
                     status = ?, updated_at = ?
                 WHERE recording_id = ? AND asset_kind = ?",
                (
                    provider_asset_id,
                    source_download_url_last_seen,
                    mime_type,
                    status,
                    now.as_str(),
                    recording_id,
                    asset_kind,
                ),
            )
            .await?;
        }

        Ok(())
    }

    pub async fn get_recording_with_audio_asset(
        &self,
        recording_id: &str,
    ) -> Result<Option<StoredRecordingWithAsset>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT
                    r.id,
                    r.meeting_id,
                    ra.source_download_url_last_seen
                FROM recordings r
                LEFT JOIN recording_assets ra
                    ON ra.recording_id = r.id AND ra.asset_kind = 'audio_mixed_mp3'
                WHERE r.id = ?
                LIMIT 1
                "#,
                params![recording_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredRecordingWithAsset {
                id: row.get::<String>(0)?,
                meeting_id: row.get::<String>(1)?,
                audio_asset: Some(StoredRecordingAudioAsset {
                    source_download_url_last_seen: row.get::<Option<String>>(2)?,
                }),
            }));
        }

        Ok(None)
    }

    pub async fn replace_transcription(
        &self,
        meeting_id: &str,
        recording_id: &str,
        provider: &str,
        model: &str,
        language: Option<&str>,
        full_text: &str,
        raw_response_json: &str,
        segments: Vec<GroqSegment>,
    ) -> Result<StoredTranscription> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let id = new_id();
        conn.execute(
            "INSERT INTO transcriptions (
                id, meeting_id, recording_id, provider, model, language, status, full_text,
                raw_response_json, started_at, completed_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                recording_id,
                provider,
                model,
                language,
                full_text,
                raw_response_json,
                now.as_str(),
                now.as_str(),
                now.as_str(),
                now.as_str(),
            ),
        )
        .await?;

        for segment in segments {
            conn.execute(
                "INSERT INTO transcript_segments (
                    id, transcription_id, seq, speaker_label, start_ms, end_ms, text, confidence_json, raw_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    new_id(),
                    id.as_str(),
                    segment.seq,
                    segment.speaker_label,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.confidence_json,
                    segment.raw_json,
                ),
            )
            .await?;
        }

        conn.execute(
            "UPDATE meetings SET processing_status = 'transcribed', updated_at = ? WHERE id = ?",
            (now.as_str(), meeting_id),
        )
        .await?;

        Ok(StoredTranscription {
            id,
            full_text: Some(full_text.to_owned()),
        })
    }

    pub async fn get_transcription(
        &self,
        transcription_id: &str,
    ) -> Result<Option<StoredTranscription>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, full_text FROM transcriptions WHERE id = ? LIMIT 1",
                params![transcription_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredTranscription {
                id: row.get::<String>(0)?,
                full_text: row.get::<Option<String>>(1)?,
            }));
        }

        Ok(None)
    }

    pub async fn replace_note(
        &self,
        meeting_id: &str,
        transcription_id: &str,
        provider: &str,
        model: &str,
        prompt_version: &str,
        note: GeneratedNote,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let note_id = new_id();
        conn.execute(
            "INSERT INTO notes (
                id, meeting_id, transcription_id, provider, model, prompt_version, status,
                title, summary_markdown, key_points_json, decisions_json, risks_json, generated_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                note_id.as_str(),
                meeting_id,
                transcription_id,
                provider,
                model,
                prompt_version,
                note.title,
                note.summary_markdown,
                serde_json::to_string(&note.key_points)?,
                serde_json::to_string(&note.decisions)?,
                serde_json::to_string(&note.risks)?,
                now.as_str(),
                now.as_str(),
                now.as_str(),
            ),
        )
        .await?;

        for (index, action_item) in note.action_items.into_iter().enumerate() {
            conn.execute(
                "INSERT INTO action_items (
                    id, note_id, meeting_id, assignee_name, assignee_email, description, due_date, priority, status, sort_order, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    new_id(),
                    note_id.as_str(),
                    meeting_id,
                    action_item.assignee_name,
                    action_item.assignee_email,
                    action_item.description,
                    action_item.due_date,
                    action_item.priority,
                    action_item.status,
                    index as i64,
                    now.as_str(),
                    now.as_str(),
                ),
            )
            .await?;
        }

        conn.execute(
            "UPDATE meetings SET latest_note_id = ?, status = 'completed', processing_status = 'completed', updated_at = ? WHERE id = ?",
            (note_id.as_str(), now.as_str(), meeting_id),
        )
        .await?;

        Ok(())
    }
}
