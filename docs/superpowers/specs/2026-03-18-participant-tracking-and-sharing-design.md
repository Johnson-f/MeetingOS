# Participant Tracking & Email Sharing

## Overview

Add participant tracking to meetings and enable sharing of meeting summaries, transcripts, and audio recordings via email. This extends the existing recording pipeline with two new capabilities:

1. **Participant tracking** — capture who attended each meeting from two sources (Google Calendar attendees and Recall.ai bot data), merge them into a unified participants list.
2. **Email sharing** — allow users to share meeting results with teammates via email (manual or auto), with a public token-based share page for viewing the full transcript and audio.

## Architecture: Pipeline Extension (Approach A)

Extends the existing job pipeline rather than introducing separate service modules. Participant extraction piggybacks on the `retrieve_bot()` call already made in `fetch_recording_media_job`. Email sharing is a new job (`send_share_emails`) chained after `generate_note_job`.

## 1. Participant Tracking

### Data Sources

**Google Calendar attendees (pre-meeting):**
- When `schedule_meeting_bots_job` dispatches a bot for a calendar-sourced meeting, copy rows from `calendar_attendees` (linked via `calendar_events.meeting_id`) into the `participants` table.
- These provide email addresses, display names, organizer status, and response status before the meeting occurs.
- Calendar-sourced participants are identified by having an `email` but no `provider_participant_id`.

**Recall.ai bot response (post-meeting):**
- `fetch_recording_media_job` already calls `retrieve_bot()`, which returns a JSON response including a `meeting_participants` array.
- Each participant has: `id` (provider participant ID), `name`, and timeline events (join/leave).
- Extract these and upsert into `participants` + `participant_events`.
- Recall-sourced participants are identified by having a `provider_participant_id`.

**Manual meetings (non-calendar):**
- For meetings created manually via `POST /api/v1/meetings`, there are no calendar attendees. Participants come only from Recall.ai bot data. No matching/merging is needed — all participants are inserted directly.

### Matching Logic

Calendar attendees have emails but may have generic display names. Recall participants have actual meeting display names but typically no emails. Matching strategy:

1. Normalize names: lowercase, trim whitespace, strip common suffixes like "(Host)".
2. For each Recall participant, attempt to match against existing calendar-sourced participants for the same meeting by case-insensitive name comparison.
3. If matched: merge into one row — keep the calendar email, update display_name to the Recall-provided name, set `provider_participant_id`, update `first_joined_at`/`last_left_at`.
4. If no match: create a new `participants` row without an email.
5. Unmatched calendar attendees remain as-is (they were invited but may not have joined).

**Limitations:** Name matching is inherently fuzzy. Calendar names may differ from Recall display names (e.g., "John Smith" vs "John S." or a phone number). This is a best-effort match. Unmatched records are not duplicates — they represent either a calendar invitee who didn't join, or a meeting joiner who wasn't on the calendar invite.

### Where It Hooks In

- `schedule_meeting_bots_job` — after dispatching a bot for a calendar meeting, copy `calendar_attendees` into `participants`.
- `fetch_recording_media_job` — after calling `retrieve_bot()`, extract participant data and upsert into `participants` + `participant_events`.

No new job type is needed for participant tracking.

## 2. Schema Changes

### New column on `meetings`

Add `auto_share_enabled` directly to the existing `CREATE TABLE meetings` statement in `SCHEMA_SQL` and bump the schema version:

```sql
auto_share_enabled INTEGER NOT NULL DEFAULT 0,
```

Per-meeting toggle. When enabled, all participants with email addresses are automatically emailed after note generation completes.

### New table: `share_tokens`

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

CREATE INDEX IF NOT EXISTS idx_share_tokens_token ON share_tokens(token);
```

- `token`: 32 random bytes, base64url-encoded. URL-safe and unguessable.
- `expires_at`: defaults to 30 days from creation.

### Existing tables used as-is

- **`participants`** — columns: id, meeting_id, recording_id, provider_participant_id, display_name, email, is_host, platform, external_identity_json, first_joined_at, last_left_at, metadata_json, created_at, updated_at. Foreign keys to meetings and recordings.
- **`participant_events`** — columns: id, meeting_id, recording_id, participant_id, event_type, absolute_at, relative_ms, payload_json, created_at. Foreign keys to meetings, recordings, participants.
- **`share_recipients`** — columns: id, meeting_id, participant_id, email, display_name, source (values: "manual", "auto"), is_selected, created_at, updated_at. Foreign key to meetings and participants. Add a UNIQUE index on `(meeting_id, email)` to prevent duplicate share entries.
- **`email_deliveries`** — columns: id, meeting_id, recipient_email, provider, status (queued/sent/failed), provider_message_id, error_message, sent_at, created_at, updated_at. Foreign key to meetings.

### New indexes

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_share_recipients_meeting_email ON share_recipients(meeting_id, email);
```

### New job constant

```rust
pub const JOB_SEND_SHARE_EMAILS: &str = "send_share_emails";
```

## 3. Email Sharing Flow

### Manual Share

- **Endpoint:** `POST /api/v1/meetings/{id}/share`
- **Auth:** Protected. Only the meeting owner can share.
- **Request body:** `{ "emails": ["a@b.com", "c@d.com"] }`
- **Validation:** Maximum 50 recipients per request. Emails must be valid format. Duplicates (same meeting + email) are silently skipped via the UNIQUE index.
- **Behavior:**
  1. Creates `share_recipients` rows with source `"manual"` for each email (ON CONFLICT ignore for duplicates).
  2. Generates a `share_tokens` row if one doesn't already exist for this meeting.
  3. Enqueues a `send_share_emails` job.

### Auto Share

- Triggered at the end of `generate_note_job` when `meetings.auto_share_enabled = 1`.
- Recipients: all `participants` for the meeting who have a non-null email address.
- Creates `share_recipients` rows with source `"auto"` (ON CONFLICT ignore — idempotent if the job retries).
- Enqueues `send_share_emails` job.

### `send_share_emails` Job Handler

1. Load the meeting, its note (summary, key points, decisions, action items), and the share token.
2. If no note exists yet, bail with a retryable error (the job queue will retry with backoff).
3. For each `share_recipients` row that does not yet have a successful `email_deliveries` entry:
   a. Compose email:
      - **Subject:** "Meeting Summary: {meeting title}"
      - **Body:** meeting title, date, summary markdown rendered to HTML (using `comrak` crate), key points as a bulleted list, action items if any.
      - **CTA:** "View full transcript & audio" link: `https://meet.tradstry.com/share/{token}`
   b. Send via Resend API.
   c. Create `email_deliveries` row with provider `"resend"`, status `"sent"` or `"failed"` (with error_message if failed).
3. Job is idempotent — skips recipients who already have a successful delivery.

### Resend Integration

- **Env var:** `RESEND_API_KEY` — optional. If not set, the Resend client returns `None` (same pattern as RecallAiClient). Share endpoints return an error if sharing is attempted without configuration.
- **API:** HTTP POST to `https://api.resend.com/emails`
- **From address:** configurable via `SHARE_FROM_EMAIL`, default `noreply@meet.tradstry.com` (requires DNS verification in Resend dashboard)
- **Implementation:** Simple `reqwest` HTTP call in the job handler. No SDK dependency needed.

## 4. Public Share Page

### Backend Route

- **Endpoint:** `GET /api/v1/share/{token}` (public, no auth)
- **Behavior:**
  1. Look up `share_tokens` by token value.
  2. If not found or `expires_at < now`: return 404.
  3. If the associated meeting has `deleted_at` set: return 404.
  4. If valid: return JSON with meeting title, date, note (summary, key points, decisions, action items), transcript segments (with speaker labels and timestamps), and a presigned audio URL (generated on the fly, short expiry).

### Frontend Page

- **Route:** `/share/[token]` (Next.js dynamic route)
- **Layout:** Standalone read-only page. No navigation bar, no login required.
- **Content:** Meeting title, date, summary, key points, action items, full transcript with speaker labels and timestamps, audio player.
- **Footer:** Simple branding.

### Security

- Tokens are 32 random bytes (base64url-encoded) — computationally unguessable.
- 30-day default expiry.
- Scoped to a single meeting.
- No enumeration or discovery possible.
- Deleted meetings return 404 even with a valid token.
- Revocation supported by deleting the token row (not in initial scope but design supports it).
- Per-meeting token (shared by all recipients). Per-recipient revocation is not supported in this initial scope.

## 5. API & Frontend Changes

### New Backend Routes

| Route | Auth | Description |
|-------|------|-------------|
| `POST /api/v1/meetings/{id}/share` | Protected | Manual share — send emails to specified addresses (max 50) |
| `GET /api/v1/meetings/{id}/participants` | Protected | List participants for a meeting |
| `GET /api/v1/share/{token}` | Public | Share page data — returns meeting summary, transcript, audio URL |

### Frontend Changes

- **Meeting detail page:** Add "Share" button opening a modal with email input field and send button. Show participants list (names, emails, join/leave status).
- **Meeting creation/scheduling:** Add "Auto-share with participants" toggle.
- **New `/share/[token]` page:** Read-only view of meeting summary, transcript, and audio player.

### Pipeline Changes

| Existing Job | Change |
|-------------|--------|
| `fetch_recording_media_job` | Add participant extraction from `retrieve_bot()` response |
| `schedule_meeting_bots_job` | Copy calendar attendees into `participants` when dispatching a bot |
| `generate_note_job` | Enqueue `send_share_emails` if `auto_share_enabled` is true |

| New Job | Description |
|---------|-------------|
| `send_share_emails` | Compose and send emails via Resend, track delivery status |

## 6. Configuration

New environment variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `RESEND_API_KEY` | No (sharing disabled without it) | Resend API key for sending transactional emails |
| `SHARE_TOKEN_EXPIRY_DAYS` | No (default: 30) | How long share links remain valid |
| `SHARE_FROM_EMAIL` | No (default: `noreply@meet.tradstry.com`) | Sender address for share emails |
