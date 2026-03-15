use anyhow::Result;
use libsql::{Connection, params};

use crate::{
    models::{
        ActionItemView, NoteView, RecallBotView, RecordingView, TranscriptSegmentView,
        TranscriptionView,
    },
    service::turso::read_operations::helpers::parse_string_list,
};

use super::super::client::TursoClient;

impl TursoClient {
    pub(crate) async fn get_recall_bot_view(
        &self,
        conn: &Connection,
        meeting_id: &str,
    ) -> Result<Option<RecallBotView>> {
        let mut rows = conn
            .query(
                "SELECT id, recall_bot_id, bot_name, join_at, status, status_sub_code, failure_detail, joined_at, left_at, created_at, updated_at FROM recall_bots WHERE meeting_id = ? ORDER BY created_at DESC LIMIT 1",
                params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(RecallBotView {
                id: row.get::<String>(0)?,
                recall_bot_id: row.get::<String>(1)?,
                bot_name: row.get::<String>(2)?,
                join_at: row.get::<Option<String>>(3)?,
                status: row.get::<String>(4)?,
                status_sub_code: row.get::<Option<String>>(5)?,
                failure_detail: row.get::<Option<String>>(6)?,
                joined_at: row.get::<Option<String>>(7)?,
                left_at: row.get::<Option<String>>(8)?,
                created_at: row.get::<String>(9)?,
                updated_at: row.get::<String>(10)?,
            }));
        }

        Ok(None)
    }

    pub(crate) async fn get_recording_view(
        &self,
        conn: &Connection,
        meeting_id: &str,
    ) -> Result<Option<RecordingView>> {
        let mut rows = conn
            .query(
                r#"
                SELECT
                    r.id,
                    r.recall_recording_id,
                    r.status,
                    r.duration_seconds,
                    r.started_at,
                    r.ended_at,
                    ra.status,
                    ra.source_download_url_last_seen
                FROM recordings r
                LEFT JOIN recording_assets ra
                    ON ra.recording_id = r.id AND ra.asset_kind = 'audio_mixed_mp3'
                WHERE r.meeting_id = ?
                ORDER BY r.created_at DESC
                LIMIT 1
                "#,
                params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(RecordingView {
                id: row.get::<String>(0)?,
                recall_recording_id: row.get::<Option<String>>(1)?,
                status: row.get::<String>(2)?,
                duration_seconds: row.get::<Option<i64>>(3)?,
                started_at: row.get::<Option<String>>(4)?,
                ended_at: row.get::<Option<String>>(5)?,
                audio_asset_status: row.get::<Option<String>>(6)?,
                audio_source_download_url_last_seen: row.get::<Option<String>>(7)?,
            }));
        }

        Ok(None)
    }

    pub(crate) async fn get_transcription_view(
        &self,
        conn: &Connection,
        meeting_id: &str,
    ) -> Result<Option<TranscriptionView>> {
        let mut rows = conn
            .query(
                "SELECT id, provider, model, language, status, full_text, started_at, completed_at, error_message FROM transcriptions WHERE meeting_id = ? ORDER BY created_at DESC LIMIT 1",
                params![meeting_id],
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let transcription_id = row.get::<String>(0)?;
        let segments = self
            .get_transcript_segments(conn, &transcription_id)
            .await?;

        Ok(Some(TranscriptionView {
            id: transcription_id,
            provider: row.get::<String>(1)?,
            model: row.get::<String>(2)?,
            language: row.get::<Option<String>>(3)?,
            status: row.get::<String>(4)?,
            full_text: row.get::<Option<String>>(5)?,
            segments,
            started_at: row.get::<Option<String>>(6)?,
            completed_at: row.get::<Option<String>>(7)?,
            error_message: row.get::<Option<String>>(8)?,
        }))
    }

    pub(crate) async fn get_transcript_segments(
        &self,
        conn: &Connection,
        transcription_id: &str,
    ) -> Result<Vec<TranscriptSegmentView>> {
        let mut rows = conn
            .query(
                "SELECT id, seq, speaker_label, start_ms, end_ms, text FROM transcript_segments WHERE transcription_id = ? ORDER BY seq ASC",
                params![transcription_id],
            )
            .await?;

        let mut segments = Vec::new();
        while let Some(row) = rows.next().await? {
            segments.push(TranscriptSegmentView {
                id: row.get::<String>(0)?,
                seq: row.get::<i64>(1)?,
                speaker_label: row.get::<Option<String>>(2)?,
                start_ms: row.get::<i64>(3)?,
                end_ms: row.get::<i64>(4)?,
                text: row.get::<String>(5)?,
            });
        }

        Ok(segments)
    }

    pub(crate) async fn get_note_view(
        &self,
        conn: &Connection,
        meeting_id: &str,
    ) -> Result<Option<NoteView>> {
        let mut rows = conn
            .query(
                "SELECT id, provider, model, prompt_version, status, title, summary_markdown, key_points_json, decisions_json, risks_json, generated_at FROM notes WHERE meeting_id = ? ORDER BY created_at DESC LIMIT 1",
                params![meeting_id],
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let note_id = row.get::<String>(0)?;
        let action_items = self.get_action_items(conn, &note_id).await?;

        Ok(Some(NoteView {
            id: note_id,
            provider: row.get::<String>(1)?,
            model: row.get::<String>(2)?,
            prompt_version: row.get::<String>(3)?,
            status: row.get::<String>(4)?,
            title: row.get::<Option<String>>(5)?,
            summary_markdown: row.get::<Option<String>>(6)?,
            key_points: parse_string_list(row.get::<Option<String>>(7)?),
            decisions: parse_string_list(row.get::<Option<String>>(8)?),
            risks: parse_string_list(row.get::<Option<String>>(9)?),
            action_items,
            generated_at: row.get::<Option<String>>(10)?,
        }))
    }

    pub(crate) async fn get_action_items(
        &self,
        conn: &Connection,
        note_id: &str,
    ) -> Result<Vec<ActionItemView>> {
        let mut rows = conn
            .query(
                "SELECT id, description, assignee_name, assignee_email, due_date, priority, status, sort_order FROM action_items WHERE note_id = ? ORDER BY sort_order ASC",
                params![note_id],
            )
            .await?;

        let mut action_items = Vec::new();
        while let Some(row) = rows.next().await? {
            action_items.push(ActionItemView {
                id: row.get::<String>(0)?,
                description: row.get::<String>(1)?,
                assignee_name: row.get::<Option<String>>(2)?,
                assignee_email: row.get::<Option<String>>(3)?,
                due_date: row.get::<Option<String>>(4)?,
                priority: row.get::<Option<String>>(5)?,
                status: row.get::<String>(6)?,
                sort_order: row.get::<i64>(7)?,
            });
        }

        Ok(action_items)
    }
}
