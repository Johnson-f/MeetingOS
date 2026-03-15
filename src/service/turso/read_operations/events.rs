use anyhow::{Context, Result};
use libsql::params;
use serde_json::Value;

use crate::service::turso::{
    client::{new_id, now_rfc3339},
    read_operations::{
        StoredJob, StoredProviderEvent, StoredRecallBot,
        helpers::{extract_first_i64, extract_first_string, query_optional_string},
    },
};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn store_recall_bot(
        &self,
        meeting_id: &str,
        recall_bot_id: &str,
        bot_name: &str,
        join_at: Option<&str>,
        status: &str,
        request_metadata_json: &str,
    ) -> Result<StoredRecallBot> {
        let conn = self.connection().await?;
        let created_at = now_rfc3339();
        let id = new_id();

        conn.execute(
            "INSERT OR IGNORE INTO recall_bots (
                id, meeting_id, recall_bot_id, bot_name, join_at, status, request_metadata_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.as_str(),
                meeting_id,
                recall_bot_id,
                bot_name,
                join_at,
                status,
                request_metadata_json,
                created_at.as_str(),
                created_at.as_str(),
            ),
        )
        .await?;

        conn.execute(
            "UPDATE meetings SET status = ?, updated_at = ? WHERE id = ?",
            ("scheduled", created_at.as_str(), meeting_id),
        )
        .await?;

        Ok(StoredRecallBot {
            id,
            meeting_id: meeting_id.to_owned(),
            recall_bot_id: recall_bot_id.to_owned(),
            status: status.to_owned(),
        })
    }

    pub async fn get_latest_recall_bot_for_meeting(
        &self,
        meeting_id: &str,
    ) -> Result<Option<StoredRecallBot>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, meeting_id, recall_bot_id, status FROM recall_bots WHERE meeting_id = ? ORDER BY created_at DESC LIMIT 1",
                params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredRecallBot {
                id: row.get::<String>(0)?,
                meeting_id: row.get::<String>(1)?,
                recall_bot_id: row.get::<String>(2)?,
                status: row.get::<String>(3)?,
            }));
        }

        Ok(None)
    }

    pub async fn store_provider_event(
        &self,
        provider: &str,
        provider_event_id: &str,
        event_type: &str,
        subject_id: Option<&str>,
        payload_json: &str,
        signature_verified: bool,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let id = new_id();

        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO provider_events (
                    id, provider, provider_event_id, event_type, subject_id, payload_json, signature_verified, received_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    id.as_str(),
                    provider,
                    provider_event_id,
                    event_type,
                    subject_id,
                    payload_json,
                    i64::from(signature_verified),
                    now.as_str(),
                ),
            )
            .await?;

        Ok(changed > 0)
    }

    pub async fn get_provider_event(
        &self,
        provider: &str,
        provider_event_id: &str,
    ) -> Result<Option<StoredProviderEvent>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT id, event_type, payload_json FROM provider_events WHERE provider = ? AND provider_event_id = ? LIMIT 1",
                params![provider, provider_event_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredProviderEvent {
                id: row.get::<String>(0)?,
                event_type: row.get::<String>(1)?,
                payload_json: row.get::<String>(2)?,
            }));
        }

        Ok(None)
    }

    pub async fn mark_provider_event_processed(
        &self,
        provider: &str,
        provider_event_id: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        conn.execute(
            "UPDATE provider_events SET processing_status = 'processed', processed_at = ?, error_message = NULL WHERE provider = ? AND provider_event_id = ?",
            (now.as_str(), provider, provider_event_id),
        )
        .await?;
        Ok(())
    }

    pub async fn enqueue_job(
        &self,
        job_type: &str,
        dedupe_key: Option<&str>,
        payload: &Value,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO jobs (
                    id, job_type, dedupe_key, payload_json, status, attempt_count, max_attempts, run_after, created_at, updated_at
                ) VALUES (?, ?, ?, ?, 'queued', 0, 8, ?, ?, ?)",
                (
                    new_id(),
                    job_type,
                    dedupe_key,
                    payload.to_string(),
                    now.as_str(),
                    now.as_str(),
                    now.as_str(),
                ),
            )
            .await?;

        Ok(changed > 0)
    }

    pub async fn lease_due_job(
        &self,
        lease_owner: &str,
        _lease_seconds: i64,
    ) -> Result<Option<StoredJob>> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let mut rows = conn
            .query(
                "SELECT id, job_type, payload_json, attempt_count, max_attempts FROM jobs WHERE status IN ('queued', 'retryable') AND run_after <= ? ORDER BY run_after ASC LIMIT 1",
                params![now.as_str()],
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let id = row.get::<String>(0)?;
        let changed = conn
            .execute(
                "UPDATE jobs SET status = 'leased', leased_at = ?, lease_owner = ?, updated_at = ? WHERE id = ? AND status IN ('queued', 'retryable')",
                (now.as_str(), lease_owner, now.as_str(), id.as_str()),
            )
            .await?;

        if changed == 0 {
            return Ok(None);
        }

        Ok(Some(StoredJob {
            id,
            job_type: row.get::<String>(1)?,
            payload_json: row.get::<String>(2)?,
            attempt_count: row.get::<i64>(3)?,
            max_attempts: row.get::<i64>(4)?,
        }))
    }

    pub async fn complete_job(&self, job_id: &str) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        conn.execute(
            "UPDATE jobs SET status = 'done', updated_at = ? WHERE id = ?",
            (now.as_str(), job_id),
        )
        .await?;
        Ok(())
    }

    pub async fn fail_job(
        &self,
        job_id: &str,
        error_message: &str,
        next_run_after: Option<String>,
        max_attempts: i64,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        conn.execute(
            "UPDATE jobs
             SET attempt_count = attempt_count + 1,
                 status = CASE WHEN attempt_count + 1 >= ? OR ? IS NULL THEN 'dead' ELSE 'retryable' END,
                 run_after = COALESCE(?, run_after),
                 last_error = ?,
                 updated_at = ?
             WHERE id = ?",
            (
                max_attempts,
                next_run_after.as_deref(),
                next_run_after.as_deref(),
                error_message,
                now.as_str(),
                job_id,
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn apply_recall_bot_event(&self, event_type: &str, payload: &Value) -> Result<()> {
        let conn = self.connection().await?;
        let recall_bot_id = extract_first_string(
            payload,
            &[
                &["data", "bot", "id"],
                &["data", "bot_id"],
                &["data", "id"],
                &["bot", "id"],
                &["id"],
            ],
        )
        .context("recall bot event missing bot id")?;
        let status = event_type.rsplit('.').next().unwrap_or(event_type);
        let status = match status {
            "in_call_recording" => "recording",
            "joining_call" => "joining",
            "done" => "done",
            "fatal" => "failed",
            other => other,
        };
        let now = now_rfc3339();
        let joined_at = if status == "recording" {
            Some(now.as_str())
        } else {
            None
        };
        let left_at = if status == "done" || status == "failed" {
            Some(now.as_str())
        } else {
            None
        };
        let failure_detail = extract_first_string(
            payload,
            &[&["data", "sub_code"], &["data", "code"], &["data", "error"]],
        );

        conn.execute(
            "UPDATE recall_bots
             SET status = ?, status_sub_code = COALESCE(?, status_sub_code), failure_detail = COALESCE(?, failure_detail),
                 joined_at = COALESCE(?, joined_at), left_at = COALESCE(?, left_at), updated_at = ?
             WHERE recall_bot_id = ?",
            (
                status,
                failure_detail.as_deref(),
                failure_detail.as_deref(),
                joined_at,
                left_at,
                now.as_str(),
                recall_bot_id.as_str(),
            ),
        )
        .await?;

        if let Some(meeting_id) = query_optional_string(
            &conn,
            "SELECT meeting_id FROM recall_bots WHERE recall_bot_id = ? LIMIT 1",
            params![recall_bot_id.as_str()],
        )
        .await?
        {
            let (meeting_status, processing_status) = match status {
                "recording" => ("recording", "capturing"),
                "joining" => ("joining", "pending"),
                "done" => ("processing", "pending_media"),
                "failed" => ("failed", "failed"),
                _ => ("scheduled", "pending"),
            };

            conn.execute(
                "UPDATE meetings
                 SET status = ?, processing_status = ?, actual_start_at = COALESCE(actual_start_at, ?), actual_end_at = CASE WHEN ? = 'processing' OR ? = 'failed' THEN ? ELSE actual_end_at END, updated_at = ?
                 WHERE id = ?",
                (
                    meeting_status,
                    processing_status,
                    joined_at,
                    meeting_status,
                    meeting_status,
                    left_at,
                    now.as_str(),
                    meeting_id.as_str(),
                ),
            )
            .await?;
        }

        Ok(())
    }

    pub async fn apply_recall_recording_event(&self, payload: &Value) -> Result<Option<String>> {
        let recall_bot_id = extract_first_string(
            payload,
            &[&["data", "bot", "id"], &["data", "bot_id"], &["bot", "id"]],
        );
        let recording_id = extract_first_string(
            payload,
            &[
                &["data", "recording", "id"],
                &["data", "recording_id"],
                &["recording", "id"],
            ],
        );
        let duration_seconds = extract_first_i64(
            payload,
            &[
                &["data", "recording", "duration_seconds"],
                &["data", "duration_seconds"],
            ],
        );
        let started_at = extract_first_string(
            payload,
            &[
                &["data", "recording", "started_at"],
                &["data", "started_at"],
            ],
        );
        let ended_at = extract_first_string(
            payload,
            &[&["data", "recording", "ended_at"], &["data", "ended_at"]],
        );

        let Some(recall_bot_id) = recall_bot_id else {
            return Ok(None);
        };

        let conn = self.connection().await?;
        let Some(meeting_id) = query_optional_string(
            &conn,
            "SELECT meeting_id FROM recall_bots WHERE recall_bot_id = ? LIMIT 1",
            params![recall_bot_id.as_str()],
        )
        .await?
        else {
            return Ok(None);
        };

        self.upsert_recording_for_bot(
            &meeting_id,
            &recall_bot_id,
            recording_id.as_deref(),
            duration_seconds,
            started_at.as_deref(),
            ended_at.as_deref(),
            "ready",
        )
        .await?;

        Ok(Some(meeting_id))
    }
}
