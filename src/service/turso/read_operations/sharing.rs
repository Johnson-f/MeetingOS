use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use libsql::params;
use rand::RngCore;

use crate::service::turso::client::{TursoClient, new_id, now_rfc3339};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PendingRecipient {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

impl TursoClient {
    pub async fn get_or_create_share_token(
        &self,
        meeting_id: &str,
        created_by_user_id: &str,
        expiry_days: u64,
    ) -> Result<String> {
        let conn = self.connection().await?;

        // Check for existing valid token
        let mut rows = conn
            .query(
                "SELECT token FROM share_tokens WHERE meeting_id = ? AND expires_at > datetime('now') LIMIT 1",
                params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let token: String = row.get(0)?;
            return Ok(token);
        }

        // Generate new token
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);

        let id = new_id();
        let created_at = now_rfc3339();
        // Calculate expiry using SQLite datetime arithmetic to avoid chrono Duration dependency
        let expires_at = format!(
            "{}",
            chrono::Utc::now()
                .checked_add_signed(chrono::Duration::days(expiry_days as i64))
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        );

        conn.execute(
            "INSERT INTO share_tokens (id, meeting_id, token, created_by_user_id, expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![id, meeting_id, token.as_str(), created_by_user_id, expires_at.as_str(), created_at.as_str()],
        )
        .await?;

        Ok(token)
    }

    pub async fn validate_share_token(&self, token: &str) -> Result<Option<String>> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                r#"
                SELECT st.meeting_id
                FROM share_tokens st
                INNER JOIN meetings m ON m.id = st.meeting_id
                WHERE st.token = ?
                  AND m.deleted_at IS NULL
                  AND st.expires_at > datetime('now')
                LIMIT 1
                "#,
                params![token],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(row.get::<String>(0)?))
        } else {
            Ok(None)
        }
    }

    pub async fn reset_email_deliveries(&self, meeting_id: &str, emails: &[String]) -> Result<()> {
        let conn = self.connection().await?;
        for email in emails {
            conn.execute(
                "DELETE FROM email_deliveries WHERE meeting_id = ? AND recipient_email = ?",
                params![meeting_id, email.as_str()],
            )
            .await?;
        }
        Ok(())
    }

    pub async fn add_share_recipients(
        &self,
        meeting_id: &str,
        recipients: Vec<(String, Option<String>, Option<String>)>,
        source: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        for (email, display_name, participant_id) in recipients {
            let id = new_id();
            conn.execute(
                "INSERT OR IGNORE INTO share_recipients (id, meeting_id, participant_id, email, display_name, source, is_selected, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
                params![
                    id,
                    meeting_id,
                    participant_id,
                    email,
                    display_name,
                    source,
                    now.as_str(),
                    now.as_str()
                ],
            )
            .await?;
        }

        Ok(())
    }

    pub async fn get_pending_share_recipients(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<PendingRecipient>> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                r#"
                SELECT sr.id, sr.email, sr.display_name
                FROM share_recipients sr
                WHERE sr.meeting_id = ?
                  AND sr.email IS NOT NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM email_deliveries ed
                      WHERE ed.meeting_id = sr.meeting_id
                        AND ed.recipient_email = sr.email
                        AND ed.status = 'sent'
                  )
                "#,
                params![meeting_id],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(PendingRecipient {
                id: row.get::<String>(0)?,
                email: row.get::<String>(1)?,
                display_name: row.get::<Option<String>>(2)?,
            });
        }

        Ok(results)
    }

    pub async fn record_email_delivery(
        &self,
        meeting_id: &str,
        recipient_email: &str,
        provider: &str,
        status: &str,
        provider_message_id: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let id = new_id();
        let now = now_rfc3339();
        let sent_at: Option<String> = if status == "sent" {
            Some(now.clone())
        } else {
            None
        };

        conn.execute(
            "INSERT INTO email_deliveries (id, meeting_id, recipient_email, provider, status, provider_message_id, error_message, sent_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                meeting_id,
                recipient_email,
                provider,
                status,
                provider_message_id,
                error_message,
                sent_at,
                now.as_str(),
                now.as_str()
            ],
        )
        .await?;

        Ok(())
    }

    pub async fn is_auto_share_enabled(&self, meeting_id: &str) -> Result<bool> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                "SELECT auto_share_enabled FROM meetings WHERE id = ? LIMIT 1",
                params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let val: i64 = row.get(0)?;
            Ok(val != 0)
        } else {
            Ok(false)
        }
    }

    #[allow(dead_code)]
    pub async fn set_auto_share_enabled(&self, meeting_id: &str, enabled: bool) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let val: i64 = if enabled { 1 } else { 0 };

        conn.execute(
            "UPDATE meetings SET auto_share_enabled = ?, updated_at = ? WHERE id = ?",
            params![val, now.as_str(), meeting_id],
        )
        .await?;

        Ok(())
    }
}
