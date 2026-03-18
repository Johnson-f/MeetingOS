# Participant Tracking & Email Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add participant tracking from Calendar + Recall.ai sources and enable email sharing of meeting summaries via Resend, with a public token-based share page.

**Architecture:** Extends the existing job pipeline. Participant extraction hooks into `fetch_recording_media_job` and `schedule_meeting_bots_job`. Email sharing adds a new `send_share_emails` job chained after `generate_note_job`. A public share page serves meeting content via token-based URLs.

**Tech Stack:** Rust/Axum backend, Turso (libSQL) database, Resend HTTP API, Next.js frontend, comrak (markdown-to-HTML)

**Spec:** `docs/superpowers/specs/2026-03-18-participant-tracking-and-sharing-design.md`

---

## Task 1: Schema Changes

**Files:**
- Modify: `src/service/turso/schema/tables/mod.rs` (SCHEMA_SQL)
- Modify: `src/service/turso/schema/logic.rs:9` (SCHEMA_VERSION)

- [ ] **Step 1: Add `auto_share_enabled` column to `meetings` table**

In `src/service/turso/schema/tables/mod.rs`, add to the `CREATE TABLE IF NOT EXISTS meetings` statement, after the `updated_at` column (before `UNIQUE` and `FOREIGN KEY` constraints):

```sql
auto_share_enabled INTEGER NOT NULL DEFAULT 0,
```

- [ ] **Step 2: Add `share_tokens` table**

In `src/service/turso/schema/tables/mod.rs`, add after the `email_deliveries` table definition:

```sql
CREATE TABLE IF NOT EXISTS share_tokens (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    created_by_user_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (created_by_user_id) REFERENCES users(id)
);
```

- [ ] **Step 3: Add new indexes**

In the indexes section at the bottom of `src/service/turso/schema/tables/mod.rs`, add:

```sql
CREATE INDEX IF NOT EXISTS idx_share_tokens_token ON share_tokens(token);
CREATE UNIQUE INDEX IF NOT EXISTS idx_share_recipients_meeting_email ON share_recipients(meeting_id, email);
```

- [ ] **Step 4: Bump schema version**

In `src/service/turso/schema/logic.rs:9`, change:

```rust
pub const SCHEMA_VERSION: &str = "0.5";
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add src/service/turso/schema/tables/mod.rs src/service/turso/schema/logic.rs
git commit -m "feat: add share_tokens table and auto_share_enabled column to meetings"
```

---

## Task 2: Resend Config & Client

**Files:**
- Modify: `src/config.rs` (add ResendConfig)
- Create: `src/service/resend/mod.rs`
- Modify: `src/service/mod.rs` (register module + add to ServiceRegistry)

- [ ] **Step 1: Add ResendConfig to config.rs**

In `src/config.rs`, add after the `GoogleOAuthConfig` struct:

```rust
#[derive(Debug, Clone)]
pub struct ResendConfig {
    pub api_key: Option<String>,
    pub from_email: String,
    pub share_token_expiry_days: u64,
}
```

Add `resend` field to `AppConfig` struct:

```rust
pub resend: ResendConfig,
```

Add to `AppConfig::from_env()`:

```rust
resend: ResendConfig {
    api_key: env::var("RESEND_API_KEY").ok(),
    from_email: env::var("SHARE_FROM_EMAIL")
        .unwrap_or_else(|_| "noreply@meet.tradstry.com".to_owned()),
    share_token_expiry_days: env::var("SHARE_TOKEN_EXPIRY_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30),
},
```

- [ ] **Step 2: Create Resend client module**

Create `src/service/resend/mod.rs`:

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;
use tracing::info;

use crate::config::ResendConfig;

#[derive(Clone)]
pub struct ResendClient {
    http: Client,
    api_key: String,
    from_email: String,
}

#[derive(Debug, Serialize)]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: &'a [&'a str],
    subject: &'a str,
    html: &'a str,
}

#[derive(Debug)]
pub struct SendEmailResult {
    pub provider_message_id: Option<String>,
}

impl ResendClient {
    pub fn new(config: &ResendConfig) -> Option<Self> {
        let api_key = config.api_key.clone()?;
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            http,
            api_key,
            from_email: config.from_email.clone(),
        })
    }

    pub async fn send_email(
        &self,
        to: &str,
        subject: &str,
        html: &str,
    ) -> Result<SendEmailResult> {
        let body = SendEmailRequest {
            from: &self.from_email,
            to: &[to],
            subject,
            html,
        };

        info!(to = %to, subject = %subject, "sending email via Resend");

        let response = self
            .http
            .post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .context("failed to send email via Resend")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Resend API returned {status}: {body}");
        }

        let json: serde_json::Value = response.json().await.unwrap_or_default();
        let provider_message_id = json
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        Ok(SendEmailResult {
            provider_message_id,
        })
    }
}
```

- [ ] **Step 3: Register Resend in ServiceRegistry**

In `src/service/mod.rs`, add:

```rust
pub mod resend;
```

Add import:

```rust
use resend::ResendClient;
```

Add field to `ServiceRegistry`:

```rust
pub resend: Option<ResendClient>,
```

In `ServiceRegistry::new()`, add after google_calendar initialization:

```rust
let resend = ResendClient::new(&config.resend);
```

Add to the `info!` macro:

```rust
resend = resend.is_some(),
```

Add to `Self { ... }`:

```rust
resend,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/service/resend/mod.rs src/service/mod.rs
git commit -m "feat: add Resend email client with optional config"
```

---

## Task 3: Participant DB Operations

**Files:**
- Modify: `src/service/turso/read_operations/mod.rs` (add participants module)
- Create: `src/service/turso/read_operations/participants.rs`

- [ ] **Step 1: Create participant DB operations**

Create `src/service/turso/read_operations/participants.rs`:

```rust
use anyhow::{Context, Result};

use crate::service::turso::client::TursoClient;

impl TursoClient {
    /// Insert a participant from calendar data (no provider_participant_id).
    pub async fn upsert_calendar_participant(
        &self,
        meeting_id: &str,
        email: &str,
        display_name: Option<&str>,
        is_host: bool,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        // Try to find existing participant by email for this meeting
        let mut rows = conn
            .query(
                "SELECT id FROM participants WHERE meeting_id = ? AND email = ?",
                libsql::params![meeting_id, email],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let existing_id = row.get::<String>(0)?;
            // Update display_name if provided
            if let Some(name) = display_name {
                conn.execute(
                    "UPDATE participants SET display_name = ?, updated_at = ? WHERE id = ?",
                    libsql::params![name, now.as_str(), existing_id.as_str()],
                )
                .await?;
            }
            return Ok(existing_id);
        }

        conn.execute(
            r#"
            INSERT INTO participants (id, meeting_id, display_name, email, is_host, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            libsql::params![
                id.as_str(),
                meeting_id,
                display_name,
                email,
                is_host as i64,
                now.as_str(),
                now.as_str()
            ],
        )
        .await
        .context("failed to insert calendar participant")?;

        Ok(id)
    }

    /// Insert or merge a participant from Recall.ai data.
    /// Attempts to match against existing calendar participants by name.
    pub async fn upsert_recall_participant(
        &self,
        meeting_id: &str,
        recording_id: Option<&str>,
        provider_participant_id: &str,
        display_name: &str,
        is_host: bool,
    ) -> Result<String> {
        let conn = self.connection().await?;
        let now = chrono::Utc::now().to_rfc3339();

        // Check if already exists by provider_participant_id
        let mut rows = conn
            .query(
                "SELECT id FROM participants WHERE meeting_id = ? AND provider_participant_id = ?",
                libsql::params![meeting_id, provider_participant_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let existing_id = row.get::<String>(0)?;
            conn.execute(
                "UPDATE participants SET display_name = ?, recording_id = COALESCE(?, recording_id), updated_at = ? WHERE id = ?",
                libsql::params![display_name, recording_id, now.as_str(), existing_id.as_str()],
            )
            .await?;
            return Ok(existing_id);
        }

        // Try fuzzy match against calendar participants (same meeting, no provider_participant_id)
        let normalized_name = display_name.trim().to_lowercase();
        let mut rows = conn
            .query(
                r#"
                SELECT id, display_name FROM participants
                WHERE meeting_id = ? AND provider_participant_id IS NULL
                "#,
                libsql::params![meeting_id],
            )
            .await?;

        let mut matched_id: Option<String> = None;
        while let Some(row) = rows.next().await? {
            let id = row.get::<String>(0)?;
            let existing_name = row.get::<Option<String>>(1)?.unwrap_or_default();
            let norm_existing = existing_name.trim().to_lowercase();
            // Simple matching: check if either name contains the other, or they are equal
            if norm_existing == normalized_name
                || norm_existing.contains(&normalized_name)
                || normalized_name.contains(&norm_existing)
            {
                matched_id = Some(id);
                break;
            }
        }

        if let Some(id) = matched_id {
            // Merge: keep email from calendar, update name and provider ID from Recall
            conn.execute(
                r#"
                UPDATE participants
                SET display_name = ?, provider_participant_id = ?, recording_id = COALESCE(?, recording_id), is_host = ?, updated_at = ?
                WHERE id = ?
                "#,
                libsql::params![
                    display_name,
                    provider_participant_id,
                    recording_id,
                    is_host as i64,
                    now.as_str(),
                    id.as_str()
                ],
            )
            .await?;
            return Ok(id);
        }

        // No match — create new participant
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            r#"
            INSERT INTO participants (id, meeting_id, recording_id, provider_participant_id, display_name, is_host, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            libsql::params![
                id.as_str(),
                meeting_id,
                recording_id,
                provider_participant_id,
                display_name,
                is_host as i64,
                now.as_str(),
                now.as_str()
            ],
        )
        .await
        .context("failed to insert recall participant")?;

        Ok(id)
    }

    /// Insert a participant event (join, leave, etc.)
    pub async fn insert_participant_event(
        &self,
        meeting_id: &str,
        recording_id: Option<&str>,
        participant_id: &str,
        event_type: &str,
        absolute_at: Option<&str>,
        relative_ms: Option<i64>,
        payload_json: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            r#"
            INSERT INTO participant_events (id, meeting_id, recording_id, participant_id, event_type, absolute_at, relative_ms, payload_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            libsql::params![
                id.as_str(),
                meeting_id,
                recording_id,
                participant_id,
                event_type,
                absolute_at,
                relative_ms,
                payload_json,
                now.as_str()
            ],
        )
        .await
        .context("failed to insert participant event")?;

        Ok(())
    }

    /// Update first_joined_at / last_left_at on a participant.
    pub async fn update_participant_timestamps(
        &self,
        participant_id: &str,
        first_joined_at: Option<&str>,
        last_left_at: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            r#"
            UPDATE participants
            SET first_joined_at = COALESCE(first_joined_at, ?),
                last_left_at = COALESCE(?, last_left_at),
                updated_at = ?
            WHERE id = ?
            "#,
            libsql::params![first_joined_at, last_left_at, now.as_str(), participant_id],
        )
        .await?;

        Ok(())
    }

    /// List participants for a meeting.
    pub async fn list_participants_for_meeting(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<ParticipantRow>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, display_name, email, is_host, provider_participant_id, first_joined_at, last_left_at
                FROM participants
                WHERE meeting_id = ?
                ORDER BY is_host DESC, display_name ASC
                "#,
                libsql::params![meeting_id],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(ParticipantRow {
                id: row.get(0)?,
                display_name: row.get(1)?,
                email: row.get(2)?,
                is_host: row.get::<i64>(3)? != 0,
                provider_participant_id: row.get(4)?,
                first_joined_at: row.get(5)?,
                last_left_at: row.get(6)?,
            });
        }

        Ok(results)
    }

    /// Get participants with emails for a meeting (for auto-share).
    pub async fn get_participants_with_emails(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, email, display_name FROM participants
                WHERE meeting_id = ? AND email IS NOT NULL AND email != ''
                "#,
                libsql::params![meeting_id],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push((
                row.get::<String>(0)?,
                row.get::<String>(1)?,
                row.get::<Option<String>>(2)?,
            ));
        }

        Ok(results)
    }
}

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
```

- [ ] **Step 2: Register the module**

In `src/service/turso/read_operations/mod.rs`, add:

```rust
pub mod participants;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/service/turso/read_operations/participants.rs src/service/turso/read_operations/mod.rs
git commit -m "feat: add participant DB operations (upsert, list, merge)"
```

---

## Task 4: Share & Email DB Operations

**Files:**
- Create: `src/service/turso/read_operations/sharing.rs`
- Modify: `src/service/turso/read_operations/mod.rs`

- [ ] **Step 1: Create sharing DB operations**

Create `src/service/turso/read_operations/sharing.rs`:

```rust
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;

use crate::service::turso::client::TursoClient;

impl TursoClient {
    /// Create a share token for a meeting, or return the existing one.
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
                "SELECT token FROM share_tokens WHERE meeting_id = ? AND expires_at > datetime('now')",
                libsql::params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(row.get::<String>(0)?);
        }

        // Generate new token
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let expires_at = (now + chrono::Duration::days(expiry_days as i64)).to_rfc3339();
        let created_at = now.to_rfc3339();

        conn.execute(
            r#"
            INSERT INTO share_tokens (id, meeting_id, token, created_by_user_id, expires_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
            libsql::params![
                id.as_str(),
                meeting_id,
                token.as_str(),
                created_by_user_id,
                expires_at.as_str(),
                created_at.as_str()
            ],
        )
        .await
        .context("failed to create share token")?;

        Ok(token)
    }

    /// Look up a share token and return the meeting_id if valid.
    pub async fn validate_share_token(&self, token: &str) -> Result<Option<String>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT st.meeting_id
                FROM share_tokens st
                JOIN meetings m ON m.id = st.meeting_id
                WHERE st.token = ? AND st.expires_at > datetime('now') AND m.deleted_at IS NULL
                "#,
                libsql::params![token],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(row.get::<String>(0)?))
        } else {
            Ok(None)
        }
    }

    /// Add share recipients (ON CONFLICT ignore for duplicates).
    pub async fn add_share_recipients(
        &self,
        meeting_id: &str,
        recipients: &[(String, Option<String>, Option<String>)], // (email, display_name, participant_id)
        source: &str,
    ) -> Result<()> {
        let conn = self.connection().await?;
        let now = chrono::Utc::now().to_rfc3339();

        for (email, display_name, participant_id) in recipients {
            let id = uuid::Uuid::new_v4().to_string();
            // Ignore duplicates via the UNIQUE index on (meeting_id, email)
            conn.execute(
                r#"
                INSERT OR IGNORE INTO share_recipients (id, meeting_id, participant_id, email, display_name, source, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                libsql::params![
                    id.as_str(),
                    meeting_id,
                    participant_id.as_deref(),
                    email.as_str(),
                    display_name.as_deref(),
                    source,
                    now.as_str(),
                    now.as_str()
                ],
            )
            .await?;
        }

        Ok(())
    }

    /// Get share recipients that have not been successfully emailed yet.
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
                libsql::params![meeting_id],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(PendingRecipient {
                id: row.get(0)?,
                email: row.get(1)?,
                display_name: row.get(2)?,
            });
        }

        Ok(results)
    }

    /// Record an email delivery attempt.
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
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let sent_at = if status == "sent" {
            Some(now.clone())
        } else {
            None
        };

        conn.execute(
            r#"
            INSERT INTO email_deliveries (id, meeting_id, recipient_email, provider, status, provider_message_id, error_message, sent_at, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            libsql::params![
                id.as_str(),
                meeting_id,
                recipient_email,
                provider,
                status,
                provider_message_id,
                error_message,
                sent_at.as_deref(),
                now.as_str(),
                now.as_str()
            ],
        )
        .await
        .context("failed to record email delivery")?;

        Ok(())
    }

    /// Check if auto_share is enabled for a meeting.
    pub async fn is_auto_share_enabled(&self, meeting_id: &str) -> Result<bool> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT auto_share_enabled FROM meetings WHERE id = ?",
                libsql::params![meeting_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(row.get::<i64>(0)? != 0)
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingRecipient {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}
```

- [ ] **Step 2: Register the module**

In `src/service/turso/read_operations/mod.rs`, add:

```rust
pub mod sharing;
```

- [ ] **Step 3: Add dependencies if missing**

Run: `cargo add rand@0.9 base64` (skip if already present in `Cargo.toml`).

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/service/turso/read_operations/sharing.rs src/service/turso/read_operations/mod.rs
git commit -m "feat: add sharing DB operations (tokens, recipients, deliveries)"
```

---

## Task 5: Participant Extraction in Pipeline

**Files:**
- Modify: `src/service/jobs/handlers.rs` (modify `fetch_recording_media_job` and `schedule_meeting_bots_job`)
- Modify: `src/service/recall_ai/client.rs` (add participant extraction method)
- Modify: `src/service/recall_ai/types.rs` (add participant types)

- [ ] **Step 1: Add Recall participant types**

In `src/service/recall_ai/types.rs`, add:

```rust
#[derive(Debug, Clone)]
pub struct RecallParticipant {
    pub id: String,
    pub name: String,
    pub is_host: bool,
    pub events: Vec<RecallParticipantEvent>,
}

#[derive(Debug, Clone)]
pub struct RecallParticipantEvent {
    pub event_type: String, // "join" or "leave"
    pub timestamp: Option<String>,
    pub relative_ms: Option<i64>,
}
```

- [ ] **Step 2: Add participant extraction to Recall client**

In `src/service/recall_ai/client.rs`, add this method to `impl RecallAiClient`:

```rust
pub fn extract_participants(&self, payload: &Value) -> Vec<super::types::RecallParticipant> {
    let participants = payload
        .get("meeting_participants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    participants
        .into_iter()
        .filter_map(|p| {
            let id = p.get("id")?.as_str()?.to_owned();
            let name = p.get("name").and_then(Value::as_str).unwrap_or("Unknown").to_owned();
            let is_host = p.get("is_host").and_then(Value::as_bool).unwrap_or(false);

            let events = p
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|e| {
                    let event_type = e.get("code")
                        .or_else(|| e.get("type"))
                        .and_then(Value::as_str)?
                        .to_owned();
                    let timestamp = e.get("timestamp").and_then(Value::as_str).map(str::to_owned);
                    let relative_ms = e.get("relative_ms").and_then(Value::as_i64);
                    Some(super::types::RecallParticipantEvent {
                        event_type,
                        timestamp,
                        relative_ms,
                    })
                })
                .collect();

            Some(super::types::RecallParticipant {
                id,
                name,
                is_host,
                events,
            })
        })
        .collect()
}
```

- [ ] **Step 3: Add participant extraction to `fetch_recording_media_job`**

In `src/service/jobs/handlers.rs`, in `fetch_recording_media_job`, after the line that calls `recall.extract_recording_media(&response)` (around line 119), add:

```rust
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

        // Update join/leave timestamps
        match event.event_type.as_str() {
            "join" => {
                services
                    .turso
                    .update_participant_timestamps(&participant_id, event.timestamp.as_deref(), None)
                    .await?;
            }
            "leave" => {
                services
                    .turso
                    .update_participant_timestamps(&participant_id, None, event.timestamp.as_deref())
                    .await?;
            }
            _ => {}
        }
    }
}
info!(meeting_id = %meeting_id, participant_count = participants.len(), "extracted participants from Recall");
```

- [ ] **Step 4: Add calendar participant copy to `schedule_meeting_bots_job`**

In `src/service/jobs/handlers.rs`, in `schedule_meeting_bots_job`, after a bot is successfully created and stored (inside the `Ok(created_bot)` match arm, after `store_recall_bot` call around line 666), add:

```rust
// Copy calendar attendees into participants
if let Ok(attendees) = services.turso.get_calendar_attendees_for_meeting(&meeting_id).await {
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
```

- [ ] **Step 5: Add `get_calendar_attendees_for_meeting` to TursoClient**

In `src/service/turso/read_operations/participants.rs`, add the `CalendarAttendeeRow` struct alongside `ParticipantRow` (outside the `impl` block):

```rust
#[derive(Debug, Clone)]
pub struct CalendarAttendeeRow {
    pub email: String,
    pub display_name: Option<String>,
    pub is_organizer: bool,
}
```

Then add this method **inside the existing `impl TursoClient`** block (not a new `impl` block):

```rust
    pub async fn get_calendar_attendees_for_meeting(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<CalendarAttendeeRow>> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT ca.email, ca.display_name, ca.is_organizer
                FROM calendar_attendees ca
                JOIN calendar_events ce ON ce.id = ca.calendar_event_id
                WHERE ce.meeting_id = ?
                "#,
                libsql::params![meeting_id],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(CalendarAttendeeRow {
                email: row.get(0)?,
                display_name: row.get(1)?,
                is_organizer: row.get::<i64>(2)? != 0,
            });
        }

        Ok(results)
    }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 7: Commit**

```bash
git add src/service/recall_ai/types.rs src/service/recall_ai/client.rs src/service/jobs/handlers.rs src/service/turso/read_operations/participants.rs
git commit -m "feat: extract participants from Recall.ai and calendar into participants table"
```

---

## Task 6: Send Share Emails Job

**Files:**
- Modify: `src/service/jobs/constants.rs`
- Modify: `src/service/jobs/handlers.rs`
- Modify: `src/service/jobs/runner.rs`

- [ ] **Step 1: Add job constant**

In `src/service/jobs/constants.rs`, add:

```rust
pub const JOB_SEND_SHARE_EMAILS: &str = "send_share_emails";
```

- [ ] **Step 2: Add comrak dependency**

Run: `cargo add comrak`

- [ ] **Step 3: Add the job handler**

In `src/service/jobs/handlers.rs`, add the import at the top:

```rust
use comrak::{markdown_to_html, Options};
```

Add the handler function:

```rust
pub(super) async fn send_share_emails_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let meeting_id = payload
        .get("meeting_id")
        .and_then(Value::as_str)
        .context("missing meeting_id")?;

    let resend = services
        .resend
        .as_ref()
        .context("Resend is not configured")?;

    // Load the share token
    let token = payload
        .get("share_token")
        .and_then(Value::as_str)
        .context("missing share_token")?;

    // Load meeting title
    let owner_id = services
        .turso
        .get_meeting_owner(meeting_id)
        .await?
        .unwrap_or_default();
    let meeting = services
        .turso
        .get_meeting_for_user(&owner_id, meeting_id)
        .await?
        .context("meeting not found")?;

    let note = meeting.note.context("meeting has no note yet — retrying")?;

    // Build email HTML
    let summary_html = note
        .summary_markdown
        .as_deref()
        .map(|md| markdown_to_html(md, &Options::default()))
        .unwrap_or_default();

    let key_points_html: String = note
        .key_points
        .iter()
        .map(|kp| format!("<li>{}</li>", html_escape(kp)))
        .collect();

    let action_items_html: String = if !note.action_items.is_empty() {
        let items: String = note
            .action_items
            .iter()
            .map(|ai| format!("<li>{}</li>", html_escape(&ai.description)))
            .collect();
        format!("<h3>Action Items</h3><ul>{}</ul>", items)
    } else {
        String::new()
    };

    let app_url = services
        .config
        .public_app_url
        .as_deref()
        .unwrap_or("https://meet.tradstry.com");
    let share_link = format!("{}/share/{}", app_url, token);

    let html_body = format!(
        r#"<h2>{title}</h2>
{summary}
{key_points_section}
{action_items}
<p><a href="{link}">View full transcript &amp; audio</a></p>
<p style="color:#888;font-size:12px">This link expires in {expiry} days.</p>"#,
        title = html_escape(&meeting.title),
        summary = summary_html,
        key_points_section = if key_points_html.is_empty() {
            String::new()
        } else {
            format!("<h3>Key Points</h3><ul>{}</ul>", key_points_html)
        },
        action_items = action_items_html,
        link = share_link,
        expiry = services.config.resend.share_token_expiry_days,
    );

    let subject = format!("Meeting Summary: {}", meeting.title);

    // Get pending recipients
    let recipients = services
        .turso
        .get_pending_share_recipients(meeting_id)
        .await?;

    info!(meeting_id = %meeting_id, recipient_count = recipients.len(), "sending share emails");

    for recipient in &recipients {
        match resend.send_email(&recipient.email, &subject, &html_body).await {
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
                info!(email = %recipient.email, "share email sent");
            }
            Err(error) => {
                services
                    .turso
                    .record_email_delivery(
                        meeting_id,
                        &recipient.email,
                        "resend",
                        "failed",
                        None,
                        Some(&error.to_string()),
                    )
                    .await?;
                warn!(email = %recipient.email, %error, "failed to send share email");
            }
        }
    }

    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
```

- [ ] **Step 4: Wire up job in runner.rs**

In `src/service/jobs/runner.rs`, add to the imports:

```rust
JOB_SEND_SHARE_EMAILS,
```

and:

```rust
send_share_emails_job,
```

Add to the `match job_type` block:

```rust
JOB_SEND_SHARE_EMAILS => send_share_emails_job(services, &payload).await,
```

- [ ] **Step 5: Chain auto-share after note generation**

In `src/service/jobs/handlers.rs`, first add `JOB_SEND_SHARE_EMAILS` to the existing import at the top of the file (line 13-16 area, the `use super::constants::{...}` block).

Then at the end of `generate_note_job` (before `Ok(())`), add:

```rust
// Auto-share: if enabled, enqueue share emails
if services.turso.is_auto_share_enabled(meeting_id).await.unwrap_or(false) {
    let participants = services
        .turso
        .get_participants_with_emails(meeting_id)
        .await
        .unwrap_or_default();

    if !participants.is_empty() {
        let recipients: Vec<(String, Option<String>, Option<String>)> = participants
            .iter()
            .map(|(id, email, name)| (email.clone(), name.clone(), Some(id.clone())))
            .collect();

        services
            .turso
            .add_share_recipients(meeting_id, &recipients, "auto")
            .await?;

        let owner_id = services
            .turso
            .get_meeting_owner(meeting_id)
            .await?
            .unwrap_or_default();

        let token = services
            .turso
            .get_or_create_share_token(
                meeting_id,
                &owner_id,
                services.config.resend.share_token_expiry_days,
            )
            .await?;

        services
            .turso
            .enqueue_job(
                JOB_SEND_SHARE_EMAILS,
                Some(&format!("share-emails-{}", meeting_id)),
                &json!({
                    "meeting_id": meeting_id,
                    "share_token": token,
                }),
            )
            .await?;

        info!(meeting_id = %meeting_id, "auto-share: enqueued share emails for {} participants", recipients.len());
    }
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 7: Commit**

```bash
git add src/service/jobs/constants.rs src/service/jobs/handlers.rs src/service/jobs/runner.rs Cargo.toml Cargo.lock
git commit -m "feat: add send_share_emails job with auto-share after note generation"
```

---

## Task 7: Backend API Routes (Share & Participants)

**Files:**
- Create: `src/routes/sharing.rs`
- Modify: `src/routes/mod.rs`
- Modify: `src/routes/router.rs`

- [ ] **Step 1: Create sharing routes**

Create `src/routes/sharing.rs`:

```rust
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::models::ApiError;

use super::{
    helpers::current_user,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ShareRequest {
    pub emails: Vec<String>,
}

pub async fn share_meeting(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
    Json(payload): Json<ShareRequest>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "POST /api/v1/meetings/{}/share", meeting_id);
    let user = current_user(&state, &jwt).await?;

    // Verify ownership
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Validate
    if payload.emails.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "emails list is empty"));
    }
    if payload.emails.len() > 50 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "maximum 50 recipients per request"));
    }
    // Basic email format validation
    for email in &payload.emails {
        if !email.contains('@') || !email.contains('.') {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid email format: {}", email),
            ));
        }
    }

    // Check Resend is configured
    if state.services.resend.is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "email sharing is not configured",
        ));
    }

    // Add recipients
    let recipients: Vec<(String, Option<String>, Option<String>)> = payload
        .emails
        .iter()
        .map(|email| (email.clone(), None, None))
        .collect();

    state
        .services
        .turso
        .add_share_recipients(&meeting_id, &recipients, "manual")
        .await?;

    // Get or create share token
    let token = state
        .services
        .turso
        .get_or_create_share_token(
            &meeting_id,
            &user.user_id,
            state.services.config.resend.share_token_expiry_days,
        )
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("failed to create share token: {e}")))?;

    // Enqueue email job
    state
        .services
        .turso
        .enqueue_job(
            crate::service::jobs::constants::JOB_SEND_SHARE_EMAILS,
            Some(&format!("share-emails-manual-{}", meeting_id)),
            &json!({
                "meeting_id": meeting_id,
                "share_token": token,
            }),
        )
        .await?;

    Ok(Json(json!({ "status": "queued", "recipient_count": payload.emails.len() })))
}

pub async fn list_participants(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(meeting_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    info!(sub = %jwt.sub, meeting_id = %meeting_id, "GET /api/v1/meetings/{}/participants", meeting_id);
    let user = current_user(&state, &jwt).await?;

    // Verify access
    state
        .services
        .turso
        .get_meeting_for_user(&user.user_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    let participants = state
        .services
        .turso
        .list_participants_for_meeting(&meeting_id)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("failed to list participants: {e}")))?;

    Ok(Json(json!({ "participants": participants })))
}

/// Public route — no auth required
pub async fn get_shared_meeting(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let meeting_id = state
        .services
        .turso
        .validate_share_token(&token)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "invalid or expired share link"))?;

    let owner_id = state
        .services
        .turso
        .get_meeting_owner(&meeting_id)
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?
        .unwrap_or_default();

    let meeting = state
        .services
        .turso
        .get_meeting_for_user(&owner_id, &meeting_id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "meeting not found"))?;

    // Generate presigned audio URL if available
    let audio_url = if let Some(storage) = state.services.storage.as_ref() {
        if let Ok(Some(asset)) = state
            .services
            .turso
            .get_audio_asset_for_meeting(&meeting_id)
            .await
        {
            if matches!(asset.status.as_deref(), Some("stored")) {
                if let Some(key) = asset.storage_key.as_deref() {
                    storage
                        .presign_audio_get(key, std::time::Duration::from_secs(600))
                        .await
                        .ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let participants = state
        .services
        .turso
        .list_participants_for_meeting(&meeting_id)
        .await
        .unwrap_or_default();

    Ok(Json(json!({
        "meeting": {
            "title": meeting.title,
            "scheduled_start_at": meeting.scheduled_start_at,
            "actual_start_at": meeting.actual_start_at,
            "actual_end_at": meeting.actual_end_at,
            "platform": meeting.platform,
        },
        "note": meeting.note,
        "transcription": meeting.transcription,
        "participants": participants,
        "audio_url": audio_url,
    })))
}
```

- [ ] **Step 2: Register module and routes**

In `src/routes/mod.rs`, add:

```rust
pub mod sharing;
```

In `src/routes/router.rs`, add import:

```rust
use super::sharing::{get_shared_meeting, list_participants, share_meeting};
```

Add to `public_routes`:

```rust
.route("/api/v1/share/{token}", get(get_shared_meeting))
```

Add to `protected_routes`:

```rust
.route("/api/v1/meetings/{meeting_id}/share", post(share_meeting))
.route("/api/v1/meetings/{meeting_id}/participants", get(list_participants))
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/routes/sharing.rs src/routes/mod.rs src/routes/router.rs
git commit -m "feat: add share, participants, and public share page API routes"
```

---

## Task 8: Frontend — Types, API Client, Hooks

**Files:**
- Modify: `frontend/src/lib/types/meetings.ts`
- Modify: `frontend/src/lib/backend_connection/client.ts`
- Create: `frontend/src/lib/hooks/use-participants-query.ts`
- Create: `frontend/src/lib/hooks/use-share-mutation.ts`
- Modify: `frontend/src/lib/hooks/index.ts`
- Modify: `frontend/src/lib/service/query-keys.ts`

- [ ] **Step 1: Add TypeScript types**

In `frontend/src/lib/types/meetings.ts`, add:

```typescript
export interface ParticipantView {
  id: string;
  display_name: string | null;
  email: string | null;
  is_host: boolean;
  provider_participant_id: string | null;
  first_joined_at: string | null;
  last_left_at: string | null;
}

export interface ParticipantsResponse {
  participants: ParticipantView[];
}

export interface ShareMeetingPayload {
  emails: string[];
}

export interface ShareMeetingResponse {
  status: string;
  recipient_count: number;
}

export interface SharedMeetingResponse {
  meeting: {
    title: string;
    scheduled_start_at: string | null;
    actual_start_at: string | null;
    actual_end_at: string | null;
    platform: string;
  };
  note: NoteView | null;
  transcription: TranscriptionView | null;
  participants: ParticipantView[];
  audio_url: string | null;
}
```

Also export from `frontend/src/lib/types/index.ts`.

- [ ] **Step 2: Add API client methods**

In `frontend/src/lib/backend_connection/client.ts`, add these methods to `BackendClient`:

```typescript
getParticipants(meetingId: string) {
  return this.request<ParticipantsResponse>(`/api/v1/meetings/${meetingId}/participants`);
}

shareMeeting(meetingId: string, payload: ShareMeetingPayload) {
  return this.request<ShareMeetingResponse>(`/api/v1/meetings/${meetingId}/share`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

getSharedMeeting(token: string) {
  return this.request<SharedMeetingResponse>(`/api/v1/share/${token}`);
}
```

Add the necessary type imports at the top of the file.

- [ ] **Step 3: Add query key**

In `frontend/src/lib/service/query-keys.ts`, add:

```typescript
participants: (meetingId: string) => ["participants", meetingId] as const,
sharedMeeting: (token: string) => ["shared-meeting", token] as const,
```

- [ ] **Step 4: Create participants hook**

Create `frontend/src/lib/hooks/use-participants-query.ts`:

```typescript
import { useQuery } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";
import { queryKeys } from "@/lib/service";

export function useParticipantsQuery(meetingId: string) {
  const client = useBackendClient();
  return useQuery({
    queryKey: queryKeys.participants(meetingId),
    queryFn: () => client.getParticipants(meetingId),
    enabled: Boolean(meetingId),
  });
}
```

- [ ] **Step 5: Create share mutation hook**

Create `frontend/src/lib/hooks/use-share-mutation.ts`:

```typescript
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useShareMutation(meetingId: string) {
  const client = useBackendClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (emails: string[]) => client.shareMeeting(meetingId, { emails }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["meetings"] });
    },
  });
}
```

- [ ] **Step 6: Export hooks**

In `frontend/src/lib/hooks/index.ts`, add:

```typescript
export { useParticipantsQuery } from "./use-participants-query";
export { useShareMutation } from "./use-share-mutation";
```

- [ ] **Step 7: Verify frontend builds**

Run: `cd frontend && npm run build`
Expected: Builds successfully (or `next lint` passes at minimum).

- [ ] **Step 8: Commit**

```bash
git add frontend/src/lib/types/ frontend/src/lib/backend_connection/client.ts frontend/src/lib/hooks/ frontend/src/lib/service/query-keys.ts
git commit -m "feat: add frontend types, API client methods, and hooks for sharing & participants"
```

---

## Task 9: Frontend — Share Modal & Participants List

**Files:**
- Create: `frontend/src/components/meetings/share-dialog.tsx`
- Create: `frontend/src/components/meetings/participants-list.tsx`
- Modify: `frontend/src/components/meetings/meeting-detail.tsx`

- [ ] **Step 1: Create share dialog component**

Create `frontend/src/components/meetings/share-dialog.tsx`:

```tsx
"use client"

import { useState } from "react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { useShareMutation } from "@/lib/hooks"

export function ShareDialog({ meetingId }: { meetingId: string }) {
  const [open, setOpen] = useState(false)
  const [emailInput, setEmailInput] = useState("")
  const [emails, setEmails] = useState<string[]>([])
  const shareMutation = useShareMutation(meetingId)

  const addEmail = () => {
    const trimmed = emailInput.trim()
    if (trimmed && !emails.includes(trimmed)) {
      setEmails([...emails, trimmed])
      setEmailInput("")
    }
  }

  const removeEmail = (email: string) => {
    setEmails(emails.filter((e) => e !== email))
  }

  const handleShare = () => {
    if (emails.length === 0) return
    shareMutation.mutate(emails, {
      onSuccess: () => {
        setEmails([])
        setOpen(false)
      },
    })
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          Share
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Share meeting summary</DialogTitle>
        </DialogHeader>
        <div className="space-y-4">
          <div className="flex gap-2">
            <Input
              placeholder="Email address"
              value={emailInput}
              onChange={(e) => setEmailInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault()
                  addEmail()
                }
              }}
            />
            <Button variant="secondary" onClick={addEmail}>
              Add
            </Button>
          </div>
          {emails.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {emails.map((email) => (
                <span
                  key={email}
                  className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-1 text-sm"
                >
                  {email}
                  <button
                    onClick={() => removeEmail(email)}
                    className="text-muted-foreground hover:text-foreground"
                  >
                    x
                  </button>
                </span>
              ))}
            </div>
          )}
          <Button
            onClick={handleShare}
            disabled={emails.length === 0 || shareMutation.isPending}
            className="w-full"
          >
            {shareMutation.isPending ? "Sending..." : `Send to ${emails.length} recipient${emails.length !== 1 ? "s" : ""}`}
          </Button>
          {shareMutation.isError && (
            <p className="text-sm text-destructive">
              {(shareMutation.error as Error)?.message ?? "Failed to share"}
            </p>
          )}
          {shareMutation.isSuccess && (
            <p className="text-sm text-green-600">Emails queued for delivery</p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 2: Create participants list component**

Create `frontend/src/components/meetings/participants-list.tsx`:

```tsx
"use client"

import { useParticipantsQuery } from "@/lib/hooks"
import { Skeleton } from "@/components/ui/skeleton"

export function ParticipantsList({ meetingId }: { meetingId: string }) {
  const { data, isLoading } = useParticipantsQuery(meetingId)

  if (isLoading) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-4 w-32" />
        <Skeleton className="h-4 w-48" />
      </div>
    )
  }

  const participants = data?.participants ?? []

  if (participants.length === 0) {
    return <p className="text-sm text-muted-foreground">No participants recorded</p>
  }

  return (
    <div className="space-y-2">
      {participants.map((p) => (
        <div key={p.id} className="flex items-center justify-between text-sm">
          <div>
            <span className="font-medium">{p.display_name ?? "Unknown"}</span>
            {p.is_host && (
              <span className="ml-2 text-xs text-muted-foreground">(Host)</span>
            )}
            {p.email && (
              <span className="ml-2 text-xs text-muted-foreground">{p.email}</span>
            )}
          </div>
          <div className="text-xs text-muted-foreground">
            {p.first_joined_at ? "Joined" : "Invited"}
          </div>
        </div>
      ))}
    </div>
  )
}
```

- [ ] **Step 3: Integrate into meeting detail page**

In `frontend/src/components/meetings/meeting-detail.tsx`, import and add the ShareDialog and ParticipantsList components. Add the ShareDialog button next to the meeting title/header area. Add a "Participants" tab or section using ParticipantsList.

The exact integration depends on the current layout — look at the existing Tabs structure and add a new `TabsTrigger` for "Participants" and corresponding `TabsContent` with `<ParticipantsList meetingId={meetingId} />`. Add `<ShareDialog meetingId={meetingId} />` in the header area next to existing action buttons.

- [ ] **Step 4: Verify frontend builds**

Run: `cd frontend && npm run build`
Expected: Builds successfully.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/meetings/share-dialog.tsx frontend/src/components/meetings/participants-list.tsx frontend/src/components/meetings/meeting-detail.tsx
git commit -m "feat: add share dialog and participants list to meeting detail page"
```

---

## Task 10: Frontend — Public Share Page

**Files:**
- Create: `frontend/src/app/share/[token]/page.tsx`

- [ ] **Step 1: Create the public share page**

First install `react-markdown`: `cd frontend && npm install react-markdown`

Create `frontend/src/app/share/[token]/page.tsx`:

```tsx
"use client"

import { use } from "react"
import { useQuery } from "@tanstack/react-query"
import ReactMarkdown from "react-markdown"
import { BackendClient } from "@/lib/backend_connection/client"
import { queryKeys } from "@/lib/service"
import { Skeleton } from "@/components/ui/skeleton"
import { Separator } from "@/components/ui/separator"

// No auth needed — public page with its own client
const publicClient = new BackendClient()

export default function SharedMeetingPage({
  params,
}: {
  params: Promise<{ token: string }>
}) {
  const { token } = use(params)

  const { data, isLoading, isError } = useQuery({
    queryKey: queryKeys.sharedMeeting(token),
    queryFn: () => publicClient.getSharedMeeting(token),
    retry: false,
  })

  if (isLoading) {
    return (
      <div className="mx-auto max-w-3xl p-8 space-y-4">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-32 w-full" />
      </div>
    )
  }

  if (isError || !data) {
    return (
      <div className="mx-auto max-w-3xl p-8">
        <h1 className="text-2xl font-bold mb-2">Link expired or not found</h1>
        <p className="text-muted-foreground">
          This share link may have expired or is invalid.
        </p>
      </div>
    )
  }

  const { meeting, note, transcription, audio_url } = data

  return (
    <div className="mx-auto max-w-3xl p-8 space-y-6">
      <header>
        <h1 className="text-2xl font-bold">{meeting.title}</h1>
        <p className="text-sm text-muted-foreground">
          {meeting.platform} &middot;{" "}
          {meeting.actual_start_at
            ? new Date(meeting.actual_start_at).toLocaleString()
            : meeting.scheduled_start_at
              ? new Date(meeting.scheduled_start_at).toLocaleString()
              : ""}
        </p>
      </header>

      {audio_url && (
        <div>
          <h2 className="text-lg font-semibold mb-2">Audio Recording</h2>
          <audio controls src={audio_url} className="w-full" />
        </div>
      )}

      {note && (
        <>
          {note.summary_markdown && (
            <div>
              <h2 className="text-lg font-semibold mb-2">Summary</h2>
              <div className="prose prose-sm dark:prose-invert max-w-none">
                <ReactMarkdown>{note.summary_markdown}</ReactMarkdown>
              </div>
            </div>
          )}

          {note.key_points.length > 0 && (
            <div>
              <h2 className="text-lg font-semibold mb-2">Key Points</h2>
              <ul className="list-disc pl-5 space-y-1 text-sm">
                {note.key_points.map((point, i) => (
                  <li key={i}>{point}</li>
                ))}
              </ul>
            </div>
          )}

          {note.action_items.length > 0 && (
            <div>
              <h2 className="text-lg font-semibold mb-2">Action Items</h2>
              <ul className="list-disc pl-5 space-y-1 text-sm">
                {note.action_items.map((item) => (
                  <li key={item.id}>
                    {item.description}
                    {item.assignee_name && (
                      <span className="text-muted-foreground"> — {item.assignee_name}</span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}

      <Separator />

      {transcription && transcription.segments.length > 0 && (
        <div>
          <h2 className="text-lg font-semibold mb-2">Transcript</h2>
          <div className="space-y-3">
            {transcription.segments.map((seg) => (
              <div key={seg.id} className="text-sm">
                <span className="font-medium text-muted-foreground">
                  {seg.speaker_label ?? "Speaker"}{" "}
                  <span className="text-xs">
                    ({Math.floor(seg.start_ms / 60000)}:{String(Math.floor((seg.start_ms % 60000) / 1000)).padStart(2, "0")})
                  </span>
                </span>
                <p>{seg.text}</p>
              </div>
            ))}
          </div>
        </div>
      )}

      <footer className="text-center text-xs text-muted-foreground pt-8">
        Powered by Meeting Bot
      </footer>
    </div>
  )
}
```

- [ ] **Step 2: Verify frontend builds**

Run: `cd frontend && npm run build`
Expected: Builds successfully.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/app/share/
git commit -m "feat: add public share page for viewing shared meeting content"
```

---

## Task 11: Auto-Share Toggle in Meeting Creation

**Files:**
- Modify: `src/models/meeting_api.rs` (add auto_share_enabled to CreateMeetingRequest and UpdateMeetingRequest)
- Modify: `src/routes/meetings.rs` (pass auto_share_enabled through)
- Modify: `frontend/src/components/new-meeting.tsx` (add toggle)
- Modify: `frontend/src/lib/types/meetings.ts` (add to payload types)

- [ ] **Step 1: Add to backend request types**

In `src/models/meeting_api.rs`, add to `CreateMeetingRequest`:

```rust
#[serde(default)]
pub auto_share_enabled: bool,
```

Add to `UpdateMeetingRequest`:

```rust
pub auto_share_enabled: Option<bool>,
```

- [ ] **Step 2: Pass through in meeting creation and update**

In `src/routes/meetings.rs` `create_meeting`, after the meeting is created, set auto_share if requested. Add after the `store_recall_bot` call (around line 151), before the final `get_meeting_for_user` call:

```rust
if payload.auto_share_enabled {
    let _ = state
        .services
        .turso
        .set_auto_share_enabled(&existing.id, true)
        .await;
}
```

In `update_meeting`, add auto_share_enabled to the update call. You'll need to add a `set_auto_share_enabled` method to TursoClient:

```rust
pub async fn set_auto_share_enabled(&self, meeting_id: &str, enabled: bool) -> Result<()> {
    let conn = self.connection().await?;
    conn.execute(
        "UPDATE meetings SET auto_share_enabled = ?, updated_at = datetime('now') WHERE id = ?",
        libsql::params![enabled as i64, meeting_id],
    )
    .await?;
    Ok(())
}
```

Add this method in `src/service/turso/read_operations/sharing.rs` inside the existing `impl TursoClient` block.

- [ ] **Step 3: Add toggle to frontend**

In `frontend/src/lib/types/meetings.ts`, add to `CreateMeetingPayload`:

```typescript
auto_share_enabled?: boolean;
```

Add to `UpdateMeetingPayload`:

```typescript
auto_share_enabled?: boolean;
```

In `frontend/src/components/new-meeting.tsx`, add a checkbox/switch labeled "Auto-share with participants" in the meeting creation form. Wire it to the `auto_share_enabled` field in the payload.

- [ ] **Step 4: Verify both build**

Run: `cargo check && cd frontend && npm run build`
Expected: Both compile successfully.

- [ ] **Step 5: Commit**

```bash
git add src/models/meeting_api.rs src/routes/meetings.rs src/service/turso/read_operations/sharing.rs frontend/src/lib/types/meetings.ts frontend/src/components/new-meeting.tsx
git commit -m "feat: add auto-share toggle to meeting creation and update"
```

---

## Task 12: End-to-End Verification

- [ ] **Step 1: Check everything compiles**

```bash
cargo check && cd frontend && npm run build
```

- [ ] **Step 2: Run cargo fmt and clippy**

```bash
cargo fmt --check && cargo clippy -- -D warnings
```

Fix any issues.

- [ ] **Step 3: Manual verification checklist**

Verify these flows work end-to-end:
1. Create a meeting — `auto_share_enabled` field accepted
2. After bot records → transcript → note pipeline, check that participants are populated
3. `GET /api/v1/meetings/{id}/participants` returns participant list
4. `POST /api/v1/meetings/{id}/share` with emails queues the job
5. `GET /api/v1/share/{token}` returns meeting data for valid tokens, 404 for invalid
6. Share page at `/share/[token]` renders correctly

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore: final formatting and cleanup for participant tracking and sharing"
```
