use anyhow::Result;
use libsql::params;

use crate::service::turso::client::{new_id, now_rfc3339};
use super::super::client::TursoClient;
use super::helpers::query_optional_string;

#[derive(Debug, Clone)]
pub struct StoredOAuthConnection {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredCalendar {
    pub id: String,
    pub provider_calendar_id: String,
    pub sync_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredCalendarEvent {
    pub id: String,
    pub meeting_id: Option<String>,
    pub scheduled_start_at: Option<String>,
}

impl TursoClient {
    pub async fn store_oauth_connection(
        &self,
        id: &str,
        workspace_id: &str,
        user_id: &str,
        provider: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        created_at: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "INSERT OR REPLACE INTO oauth_connections (id, workspace_id, user_id, provider, access_token_encrypted, refresh_token_encrypted, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'connected', ?, ?)",
            (id, workspace_id, user_id, provider, access_token, refresh_token, created_at, created_at),
        ).await?;
        Ok(())
    }

    pub async fn get_oauth_connection(
        &self,
        user_id: &str,
        provider: &str,
    ) -> Result<Option<StoredOAuthConnection>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, user_id, workspace_id, access_token_encrypted, refresh_token_encrypted FROM oauth_connections WHERE user_id = ? AND provider = ? AND status = 'connected' LIMIT 1",
                params![user_id, provider],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredOAuthConnection {
                id: row.get::<String>(0)?,
                user_id: row.get::<String>(1)?,
                workspace_id: row.get::<String>(2)?,
                access_token: row.get::<Option<String>>(3)?,
                refresh_token: row.get::<Option<String>>(4)?,
            }));
        }
        Ok(None)
    }

    pub async fn get_all_active_oauth_connections(
        &self,
        provider: &str,
    ) -> Result<Vec<StoredOAuthConnection>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, user_id, workspace_id, access_token_encrypted, refresh_token_encrypted FROM oauth_connections WHERE provider = ? AND status = 'connected'",
                params![provider],
            )
            .await?;

        let mut connections = Vec::new();
        while let Some(row) = rows.next().await? {
            connections.push(StoredOAuthConnection {
                id: row.get::<String>(0)?,
                user_id: row.get::<String>(1)?,
                workspace_id: row.get::<String>(2)?,
                access_token: row.get::<Option<String>>(3)?,
                refresh_token: row.get::<Option<String>>(4)?,
            });
        }
        Ok(connections)
    }

    pub async fn update_oauth_tokens(
        &self,
        connection_id: &str,
        access_token: &str,
        refresh_token: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        conn.execute(
            "UPDATE oauth_connections SET access_token_encrypted = ?, refresh_token_encrypted = COALESCE(?, refresh_token_encrypted), updated_at = ? WHERE id = ?",
            (access_token, refresh_token, now.as_str(), connection_id),
        ).await?;
        Ok(())
    }

    pub async fn delete_oauth_connection(&self, connection_id: &str) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "UPDATE oauth_connections SET status = 'disconnected', updated_at = ? WHERE id = ?",
            (now_rfc3339(), connection_id),
        ).await?;
        Ok(())
    }

    pub async fn store_calendar(
        &self,
        oauth_connection_id: &str,
        provider: &str,
        provider_calendar_id: &str,
        name: &str,
        is_primary: bool,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let id = new_id();
        let now = now_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO calendar_calendars (id, oauth_connection_id, provider, provider_calendar_id, name, is_primary, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'active', ?, ?)",
            (id.as_str(), oauth_connection_id, provider, provider_calendar_id, name, i64::from(is_primary), now.as_str(), now.as_str()),
        ).await?;
        Ok(id)
    }

    pub async fn list_calendar_calendars(
        &self,
        oauth_connection_id: &str,
    ) -> Result<Vec<StoredCalendar>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, provider_calendar_id, sync_cursor FROM calendar_calendars WHERE oauth_connection_id = ? AND status = 'active'",
                params![oauth_connection_id],
            )
            .await?;

        let mut calendars = Vec::new();
        while let Some(row) = rows.next().await? {
            calendars.push(StoredCalendar {
                id: row.get::<String>(0)?,
                provider_calendar_id: row.get::<String>(1)?,
                sync_cursor: row.get::<Option<String>>(2)?,
            });
        }
        Ok(calendars)
    }

    pub async fn update_calendar_sync_cursor(
        &self,
        calendar_id: &str,
        sync_cursor: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "UPDATE calendar_calendars SET sync_cursor = ?, last_synced_at = ?, updated_at = ? WHERE id = ?",
            (sync_cursor, now_rfc3339(), now_rfc3339(), calendar_id),
        ).await?;
        Ok(())
    }

    pub async fn update_calendar_watch(
        &self,
        oauth_connection_id: &str,
        provider_calendar_id: &str,
        channel_id: &str,
        resource_id: &str,
        expiration: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "UPDATE calendar_calendars SET watch_channel_id = ?, watch_resource_id = ?, watch_expires_at = ?, updated_at = ? WHERE oauth_connection_id = ? AND provider_calendar_id = ?",
            (channel_id, resource_id, expiration, now_rfc3339(), oauth_connection_id, provider_calendar_id),
        ).await?;
        Ok(())
    }

    pub async fn upsert_calendar_event(
        &self,
        workspace_id: &str,
        calendar_id: &str,
        provider: &str,
        provider_event_id: &str,
        title: &str,
        meeting_url: Option<&str>,
        starts_at: Option<&str>,
        ends_at: Option<&str>,
        organizer_email: Option<&str>,
        raw_json: &str,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        // Check if exists
        if let Some(existing_id) = query_optional_string(
            &conn,
            "SELECT id FROM calendar_events WHERE calendar_id = ? AND provider_event_id = ? LIMIT 1",
            params![calendar_id, provider_event_id],
        ).await? {
            conn.execute(
                "UPDATE calendar_events SET title = ?, meeting_url = ?, starts_at = ?, ends_at = ?, organizer_email = ?, raw_json = ?, updated_at = ? WHERE id = ?",
                (title, meeting_url, starts_at, ends_at, organizer_email, raw_json, now.as_str(), existing_id.as_str()),
            ).await?;
            return Ok(existing_id);
        }

        let id = new_id();
        conn.execute(
            "INSERT INTO calendar_events (id, workspace_id, calendar_id, provider, provider_event_id, title, meeting_url, starts_at, ends_at, organizer_email, raw_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (id.as_str(), workspace_id, calendar_id, provider, provider_event_id, title, meeting_url, starts_at, ends_at, organizer_email, raw_json, now.as_str(), now.as_str()),
        ).await?;
        Ok(id)
    }

    pub async fn get_calendar_event_by_provider_id(
        &self,
        calendar_id: &str,
        provider_event_id: &str,
    ) -> Result<Option<StoredCalendarEvent>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, meeting_id, starts_at FROM calendar_events WHERE calendar_id = ? AND provider_event_id = ? LIMIT 1",
                params![calendar_id, provider_event_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredCalendarEvent {
                id: row.get::<String>(0)?,
                meeting_id: row.get::<Option<String>>(1)?,
                scheduled_start_at: row.get::<Option<String>>(2)?,
            }));
        }
        Ok(None)
    }

    pub async fn get_meeting_for_calendar_event(
        &self,
        calendar_event_id: &str,
    ) -> Result<Option<StoredCalendarEvent>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT ce.id, ce.meeting_id, m.scheduled_start_at FROM calendar_events ce LEFT JOIN meetings m ON m.id = ce.meeting_id WHERE ce.id = ? AND ce.meeting_id IS NOT NULL LIMIT 1",
                params![calendar_event_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredCalendarEvent {
                id: row.get::<String>(0)?,
                meeting_id: row.get::<Option<String>>(1)?,
                scheduled_start_at: row.get::<Option<String>>(2)?,
            }));
        }
        Ok(None)
    }

    pub async fn link_calendar_event_to_meeting(
        &self,
        calendar_event_id: &str,
        meeting_id: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "UPDATE calendar_events SET meeting_id = ?, updated_at = ? WHERE id = ?",
            (meeting_id, now_rfc3339(), calendar_event_id),
        ).await?;
        Ok(())
    }

    pub async fn delete_calendar_event(&self, calendar_event_id: &str) -> Result<()> {
        let conn = self.connection().await?;
        conn.execute(
            "DELETE FROM calendar_events WHERE id = ?",
            params![calendar_event_id],
        ).await?;
        Ok(())
    }
}
