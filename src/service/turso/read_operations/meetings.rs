use anyhow::Result;
use libsql::{Connection, params};

use crate::{
    models::{MeetingDetail, MeetingListItem},
    service::turso::{
        client::{new_id, now_rfc3339},
        read_operations::{
            MeetingDraft, UpsertMeetingResult, UserContext, helpers::query_optional_string,
        },
    },
};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn create_or_get_meeting(
        &self,
        user: &UserContext,
        draft: &MeetingDraft,
    ) -> Result<UpsertMeetingResult> {
        let conn = self.connection().await?;

        if let Some(meeting_id) = query_optional_string(
            &conn,
            "SELECT id FROM meetings WHERE workspace_id = ? AND dedup_key = ? AND deleted_at IS NULL LIMIT 1",
            (user.workspace_id.as_str(), draft.dedup_key.as_str()),
        )
        .await?
        {
            self.attach_meeting_access(&conn, &meeting_id, &user.user_id).await?;
            return Ok(UpsertMeetingResult {
                meeting_id,
                created: false,
            });
        }

        let meeting_id = new_id();
        let created_at = now_rfc3339();
        conn.execute(
            "INSERT INTO meetings (
                id, workspace_id, created_by_user_id, source, title, normalized_meeting_url,
                original_meeting_url, platform, meeting_time_mode, dedup_key, scheduled_start_at, status,
                processing_status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'draft', 'pending', ?, ?)",
            (
                meeting_id.as_str(),
                user.workspace_id.as_str(),
                user.user_id.as_str(),
                draft.source.as_str(),
                draft.title.as_str(),
                draft.normalized_meeting_url.as_str(),
                draft.original_meeting_url.as_str(),
                draft.platform.as_str(),
                draft.meeting_time_mode.as_str(),
                draft.dedup_key.as_str(),
                draft.scheduled_start_at.as_str(),
                created_at.as_str(),
                created_at.as_str(),
            ),
        )
        .await?;
        self.attach_meeting_access(&conn, &meeting_id, &user.user_id)
            .await?;

        Ok(UpsertMeetingResult {
            meeting_id,
            created: true,
        })
    }

    pub async fn list_meetings_for_user(
        &self,
        user_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<MeetingListItem>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT
                    m.id,
                    m.title,
                    m.platform,
                    m.meeting_time_mode,
                    m.status,
                    m.processing_status,
                    m.scheduled_start_at,
                    m.actual_start_at,
                    m.actual_end_at,
                    rb.recall_bot_id,
                    n.summary_markdown,
                    m.created_at,
                    m.updated_at
                FROM meetings m
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                LEFT JOIN recall_bots rb ON rb.id = (
                    SELECT id FROM recall_bots WHERE meeting_id = m.id ORDER BY created_at DESC LIMIT 1
                )
                LEFT JOIN notes n ON n.id = m.latest_note_id
                WHERE ma.user_id = ? AND m.deleted_at IS NULL
                ORDER BY COALESCE(m.scheduled_start_at, m.created_at) DESC
                LIMIT ? OFFSET ?
                "#,
                params![user_id, limit as i64, offset as i64],
            )
            .await?;

        let mut meetings = Vec::new();
        while let Some(row) = rows.next().await? {
            meetings.push(MeetingListItem {
                id: row.get::<String>(0)?,
                title: row.get::<String>(1)?,
                platform: row.get::<String>(2)?,
                meeting_time_mode: row.get::<String>(3)?,
                status: row.get::<String>(4)?,
                processing_status: row.get::<String>(5)?,
                scheduled_start_at: row.get::<Option<String>>(6)?,
                actual_start_at: row.get::<Option<String>>(7)?,
                actual_end_at: row.get::<Option<String>>(8)?,
                recall_bot_id: row.get::<Option<String>>(9)?,
                latest_note_summary: row.get::<Option<String>>(10)?,
                created_at: row.get::<String>(11)?,
                updated_at: row.get::<String>(12)?,
            });
        }

        Ok(meetings)
    }

    pub async fn get_meeting_for_user(
        &self,
        user_id: &str,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetail>> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                r#"
                SELECT
                    m.id,
                    m.title,
                    m.platform,
                    m.source,
                    m.meeting_time_mode,
                    m.original_meeting_url,
                    m.normalized_meeting_url,
                    m.dedup_key,
                    m.status,
                    m.processing_status,
                    m.scheduled_start_at,
                    m.scheduled_end_at,
                    m.actual_start_at,
                    m.actual_end_at,
                    m.deleted_at,
                    m.created_at,
                    m.updated_at
                FROM meetings m
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                WHERE ma.user_id = ? AND m.id = ?
                LIMIT 1
                "#,
                params![user_id, meeting_id],
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let meeting_id = row.get::<String>(0)?;
        let mut detail = MeetingDetail {
            id: meeting_id.clone(),
            title: row.get::<String>(1)?,
            platform: row.get::<String>(2)?,
            source: row.get::<String>(3)?,
            meeting_time_mode: row.get::<String>(4)?,
            original_meeting_url: row.get::<String>(5)?,
            normalized_meeting_url: row.get::<String>(6)?,
            dedup_key: row.get::<String>(7)?,
            status: row.get::<String>(8)?,
            processing_status: row.get::<String>(9)?,
            scheduled_start_at: row.get::<Option<String>>(10)?,
            scheduled_end_at: row.get::<Option<String>>(11)?,
            actual_start_at: row.get::<Option<String>>(12)?,
            actual_end_at: row.get::<Option<String>>(13)?,
            deleted_at: row.get::<Option<String>>(14)?,
            created_at: row.get::<String>(15)?,
            updated_at: row.get::<String>(16)?,
            bot: self.get_recall_bot_view(&conn, &meeting_id).await?,
            recording: self.get_recording_view(&conn, &meeting_id).await?,
            transcription: self.get_transcription_view(&conn, &meeting_id).await?,
            note: self.get_note_view(&conn, &meeting_id).await?,
        };

        if let Some(note) = &detail.note {
            detail.processing_status = if note.status == "ready" {
                "completed".to_owned()
            } else {
                detail.processing_status
            };
        }

        Ok(Some(detail))
    }

    pub async fn update_meeting_status(
        &self,
        meeting_id: &str,
        status: &str,
        processing_status: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let updated_at = now_rfc3339();
        conn.execute(
            "UPDATE meetings SET status = ?, processing_status = COALESCE(?, processing_status), updated_at = ? WHERE id = ?",
            (status, processing_status, updated_at.as_str(), meeting_id),
        )
        .await?;
        Ok(())
    }

    pub async fn soft_delete_meeting_for_user(
        &self,
        user_id: &str,
        meeting_id: &str,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let allowed = query_optional_string(
            &conn,
            "SELECT m.id FROM meetings m INNER JOIN meeting_access ma ON ma.meeting_id = m.id WHERE ma.user_id = ? AND m.id = ? LIMIT 1",
            (user_id, meeting_id),
        )
        .await?
        .is_some();

        if !allowed {
            return Ok(false);
        }

        let deleted_at = now_rfc3339();
        conn.execute(
            "UPDATE meetings SET deleted_at = ?, status = 'deleted', updated_at = ? WHERE id = ?",
            (deleted_at.as_str(), deleted_at.as_str(), meeting_id),
        )
        .await?;

        Ok(true)
    }

    pub(crate) async fn attach_meeting_access(
        &self,
        conn: &Connection,
        meeting_id: &str,
        user_id: &str,
    ) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO meeting_access (id, meeting_id, user_id, created_at) VALUES (?, ?, ?, ?)",
            (new_id(), meeting_id, user_id, now_rfc3339()),
        )
        .await?;
        Ok(())
    }
}
