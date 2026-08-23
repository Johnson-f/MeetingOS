use anyhow::{Context, Result};
use libsql::params;

use super::super::client::TursoClient;
use super::helpers::query_optional_string;
use crate::service::turso::client::{new_id, now_rfc3339};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ParticipantRow {
    pub id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_host: bool,
    pub provider_participant_id: Option<String>,
    pub first_joined_at: Option<String>,
    pub last_left_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CalendarAttendeeRow {
    pub email: String,
    pub display_name: Option<String>,
    pub is_organizer: bool,
}

impl TursoClient {
    /// Insert a participant from calendar data (no provider_participant_id).
    /// If a participant with same meeting_id+email already exists, update display_name
    /// and return the existing id.
    pub async fn upsert_calendar_participant(
        &self,
        meeting_id: &str,
        email: &str,
        display_name: Option<&str>,
        is_host: bool,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        // Check if a participant with same meeting_id+email already exists
        if let Some(existing_id) = query_optional_string(
            &conn,
            "SELECT id FROM participants WHERE meeting_id = ? AND email = ? LIMIT 1",
            params![meeting_id, email],
        )
        .await?
        {
            conn.execute(
                "UPDATE participants SET display_name = COALESCE(?, display_name), updated_at = ? WHERE id = ?",
                (display_name, now.as_str(), existing_id.as_str()),
            )
            .await
            .context("failed to update calendar participant")?;
            return Ok(existing_id);
        }

        let id = new_id();
        conn.execute(
            "INSERT INTO participants (id, meeting_id, email, display_name, is_host, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                email,
                display_name,
                i64::from(is_host),
                now.as_str(),
                now.as_str(),
            ),
        )
        .await
        .context("failed to insert calendar participant")?;

        Ok(id)
    }

    /// Insert or merge a participant from Recall.ai.
    /// Checks by provider_participant_id first; if not found, attempts fuzzy name match
    /// against existing calendar participants (same meeting, no provider_participant_id).
    /// If matched, merges (keeps email, updates name+provider_id). Otherwise creates new row.
    pub async fn upsert_recall_participant(
        &self,
        meeting_id: &str,
        recording_id: Option<&str>,
        provider_participant_id: &str,
        display_name: &str,
        is_host: bool,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        // Check if already exists by provider_participant_id
        if let Some(existing_id) = query_optional_string(
            &conn,
            "SELECT id FROM participants WHERE meeting_id = ? AND provider_participant_id = ? LIMIT 1",
            params![meeting_id, provider_participant_id],
        )
        .await?
        {
            conn.execute(
                "UPDATE participants SET display_name = ?, is_host = ?, recording_id = COALESCE(?, recording_id), updated_at = ? WHERE id = ?",
                (display_name, i64::from(is_host), recording_id, now.as_str(), existing_id.as_str()),
            )
            .await
            .context("failed to update recall participant")?;
            return Ok(existing_id);
        }

        // Attempt fuzzy name match against calendar participants (no provider_participant_id)
        let mut rows = conn
            .query(
                "SELECT id, display_name FROM participants WHERE meeting_id = ? AND provider_participant_id IS NULL",
                params![meeting_id],
            )
            .await
            .context("failed to query calendar participants for fuzzy match")?;

        let normalized_new = display_name.to_lowercase();
        let normalized_new = normalized_new.trim();

        let mut matched_id: Option<String> = None;
        while let Some(row) = rows.next().await? {
            let candidate_id: String = row.get(0)?;
            let candidate_name: Option<String> = row.get(1)?;
            if let Some(name) = candidate_name {
                let normalized_candidate = name.to_lowercase();
                let normalized_candidate = normalized_candidate.trim();
                if normalized_candidate.contains(normalized_new)
                    || normalized_new.contains(normalized_candidate)
                {
                    matched_id = Some(candidate_id);
                    break;
                }
            }
        }

        if let Some(existing_id) = matched_id {
            // Merge: keep email, update name + provider_id
            conn.execute(
                "UPDATE participants SET provider_participant_id = ?, display_name = ?, is_host = ?, recording_id = COALESCE(?, recording_id), updated_at = ? WHERE id = ?",
                (provider_participant_id, display_name, i64::from(is_host), recording_id, now.as_str(), existing_id.as_str()),
            )
            .await
            .context("failed to merge recall participant")?;
            return Ok(existing_id);
        }

        // No match — create new row
        let id = new_id();
        conn.execute(
            "INSERT INTO participants (id, meeting_id, recording_id, provider_participant_id, display_name, is_host, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                recording_id,
                provider_participant_id,
                display_name,
                i64::from(is_host),
                now.as_str(),
                now.as_str(),
            ),
        )
        .await
        .context("failed to insert recall participant")?;

        Ok(id)
    }

    /// Insert a participant event.
    pub async fn insert_participant_event(
        &self,
        meeting_id: &str,
        recording_id: Option<&str>,
        participant_id: &str,
        event_type: &str,
        absolute_at: Option<&str>,
        relative_ms: Option<i64>,
        payload_json: &str,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let id = new_id();
        let now = now_rfc3339();

        conn.execute(
            "INSERT INTO participant_events (id, meeting_id, recording_id, participant_id, event_type, absolute_at, relative_ms, payload_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                recording_id,
                participant_id,
                event_type,
                absolute_at,
                relative_ms,
                payload_json,
                now.as_str(),
            ),
        )
        .await
        .context("failed to insert participant event")?;

        Ok(id)
    }

    /// Update first_joined_at/last_left_at for a participant.
    /// Uses COALESCE to only set first_joined_at if currently NULL.
    /// Always updates last_left_at with the new value.
    pub async fn update_participant_timestamps(
        &self,
        participant_id: &str,
        first_joined_at: &str,
        last_left_at: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        conn.execute(
            "UPDATE participants SET first_joined_at = COALESCE(first_joined_at, ?), last_left_at = ?, updated_at = ? WHERE id = ?",
            (first_joined_at, last_left_at, now.as_str(), participant_id),
        )
        .await
        .context("failed to update participant timestamps")?;

        Ok(())
    }

    /// List all participants for a meeting. Ordered by is_host DESC, display_name ASC.
    pub async fn list_participants_for_meeting(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<ParticipantRow>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, display_name, email, is_host, provider_participant_id, first_joined_at, last_left_at FROM participants WHERE meeting_id = ? ORDER BY is_host DESC, display_name ASC",
                params![meeting_id],
            )
            .await
            .context("failed to list participants for meeting")?;

        let mut participants = Vec::new();
        while let Some(row) = rows.next().await? {
            participants.push(ParticipantRow {
                id: row.get::<String>(0)?,
                display_name: row.get::<Option<String>>(1)?,
                email: row.get::<Option<String>>(2)?,
                is_host: row.get::<i64>(3)? != 0,
                provider_participant_id: row.get::<Option<String>>(4)?,
                first_joined_at: row.get::<Option<String>>(5)?,
                last_left_at: row.get::<Option<String>>(6)?,
            });
        }

        Ok(participants)
    }

    /// Get participants with non-null emails for a meeting.
    /// Returns Vec of (id, email, display_name) tuples.
    pub async fn get_participants_with_emails(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, email, display_name FROM participants WHERE meeting_id = ? AND email IS NOT NULL",
                params![meeting_id],
            )
            .await
            .context("failed to get participants with emails")?;

        let mut participants = Vec::new();
        while let Some(row) = rows.next().await? {
            participants.push((
                row.get::<String>(0)?,
                row.get::<String>(1)?,
                row.get::<Option<String>>(2)?,
            ));
        }

        Ok(participants)
    }

    /// Get calendar attendees for a meeting via JOIN on calendar_events.
    pub async fn get_calendar_attendees_for_meeting(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<CalendarAttendeeRow>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT ca.email, ca.display_name, ca.is_organizer FROM calendar_attendees ca JOIN calendar_events ce ON ce.id = ca.calendar_event_id WHERE ce.meeting_id = ?",
                params![meeting_id],
            )
            .await
            .context("failed to get calendar attendees for meeting")?;

        let mut attendees = Vec::new();
        while let Some(row) = rows.next().await? {
            attendees.push(CalendarAttendeeRow {
                email: row.get::<String>(0)?,
                display_name: row.get::<Option<String>>(1)?,
                is_organizer: row.get::<i64>(2)? != 0,
            });
        }

        Ok(attendees)
    }
}
