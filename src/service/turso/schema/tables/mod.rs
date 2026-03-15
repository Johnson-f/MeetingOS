/// Define your schema here. Bump SCHEMA_VERSION in logic.rs when you change this.
pub const SCHEMA_SQL: &str = r#"
-- rename_table: meetings -> meetings_legacy_v0

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    clerk_user_id TEXT NOT NULL UNIQUE,
    primary_email TEXT,
    display_name TEXT,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    owner_user_id TEXT NOT NULL,
    plan_tier TEXT NOT NULL DEFAULT 'free',
    billing_status TEXT NOT NULL DEFAULT 'inactive',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (owner_user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS workspace_members (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'owner',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL,
    UNIQUE (workspace_id, user_id),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id),
    FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS meetings (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    created_by_user_id TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    title TEXT NOT NULL,
    normalized_meeting_url TEXT NOT NULL,
    original_meeting_url TEXT NOT NULL,
    platform TEXT NOT NULL,
    meeting_time_mode TEXT NOT NULL DEFAULT 'future' CHECK (meeting_time_mode IN ('future', 'starting_now', 'already_started')),
    dedup_key TEXT NOT NULL,
    scheduled_start_at TEXT,
    scheduled_end_at TEXT,
    actual_start_at TEXT,
    actual_end_at TEXT,
    status TEXT NOT NULL DEFAULT 'draft',
    processing_status TEXT NOT NULL DEFAULT 'pending',
    latest_note_id TEXT,
    deleted_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (workspace_id, dedup_key),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id),
    FOREIGN KEY (created_by_user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS meeting_access (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    access_role TEXT NOT NULL DEFAULT 'owner',
    created_at TEXT NOT NULL,
    UNIQUE (meeting_id, user_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS recall_bots (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recall_bot_id TEXT NOT NULL UNIQUE,
    recall_region TEXT,
    bot_name TEXT NOT NULL,
    join_at TEXT,
    status TEXT NOT NULL DEFAULT 'requested',
    status_sub_code TEXT,
    failure_detail TEXT,
    request_metadata_json TEXT,
    joined_at TEXT,
    left_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id)
);

CREATE TABLE IF NOT EXISTS recordings (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recall_bot_id TEXT,
    recall_recording_id TEXT UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    duration_seconds INTEGER,
    started_at TEXT,
    ended_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (recall_bot_id) REFERENCES recall_bots(recall_bot_id)
);

CREATE TABLE IF NOT EXISTS recording_assets (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL,
    asset_kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_asset_id TEXT,
    storage_bucket TEXT,
    storage_key TEXT,
    mime_type TEXT,
    byte_size INTEGER,
    checksum_sha256 TEXT,
    source_download_url_last_seen TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (recording_id, asset_kind),
    FOREIGN KEY (recording_id) REFERENCES recordings(id)
);

CREATE TABLE IF NOT EXISTS participants (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recording_id TEXT,
    provider_participant_id TEXT,
    display_name TEXT,
    email TEXT,
    is_host INTEGER NOT NULL DEFAULT 0,
    platform TEXT,
    external_identity_json TEXT,
    first_joined_at TEXT,
    last_left_at TEXT,
    metadata_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (recording_id) REFERENCES recordings(id)
);

CREATE TABLE IF NOT EXISTS participant_events (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recording_id TEXT,
    participant_id TEXT,
    event_type TEXT NOT NULL,
    absolute_at TEXT,
    relative_ms INTEGER,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (recording_id) REFERENCES recordings(id),
    FOREIGN KEY (participant_id) REFERENCES participants(id)
);

CREATE TABLE IF NOT EXISTS transcriptions (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    language TEXT,
    status TEXT NOT NULL DEFAULT 'queued',
    provider_job_id TEXT,
    full_text TEXT,
    raw_response_json TEXT,
    started_at TEXT,
    completed_at TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (recording_id) REFERENCES recordings(id)
);

CREATE TABLE IF NOT EXISTS transcript_segments (
    id TEXT PRIMARY KEY,
    transcription_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    speaker_label TEXT,
    participant_id TEXT,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    confidence_json TEXT,
    raw_json TEXT,
    UNIQUE (transcription_id, seq),
    FOREIGN KEY (transcription_id) REFERENCES transcriptions(id),
    FOREIGN KEY (participant_id) REFERENCES participants(id)
);

CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    transcription_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    title TEXT,
    summary_markdown TEXT,
    key_points_json TEXT,
    decisions_json TEXT,
    risks_json TEXT,
    generated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (transcription_id) REFERENCES transcriptions(id)
);

CREATE TABLE IF NOT EXISTS action_items (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    assignee_participant_id TEXT,
    assignee_name TEXT,
    assignee_email TEXT,
    description TEXT NOT NULL,
    due_date TEXT,
    priority TEXT,
    status TEXT NOT NULL DEFAULT 'open',
    source_segment_id TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (note_id) REFERENCES notes(id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (assignee_participant_id) REFERENCES participants(id),
    FOREIGN KEY (source_segment_id) REFERENCES transcript_segments(id)
);

CREATE TABLE IF NOT EXISTS provider_events (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    subject_id TEXT,
    payload_json TEXT NOT NULL,
    signature_verified INTEGER NOT NULL DEFAULT 0,
    received_at TEXT NOT NULL,
    processed_at TEXT,
    processing_status TEXT NOT NULL DEFAULT 'queued',
    error_message TEXT,
    UNIQUE (provider, provider_event_id)
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    job_type TEXT NOT NULL,
    dedupe_key TEXT,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    run_after TEXT NOT NULL,
    leased_at TEXT,
    lease_owner TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS oauth_connections (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_account_id TEXT,
    email TEXT,
    access_token_encrypted TEXT,
    refresh_token_encrypted TEXT,
    scopes_json TEXT,
    token_expires_at TEXT,
    status TEXT NOT NULL DEFAULT 'disconnected',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id),
    FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS calendar_calendars (
    id TEXT PRIMARY KEY,
    oauth_connection_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_calendar_id TEXT NOT NULL,
    name TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    watch_channel_id TEXT,
    watch_resource_id TEXT,
    watch_expires_at TEXT,
    sync_cursor TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    last_synced_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (oauth_connection_id, provider_calendar_id),
    FOREIGN KEY (oauth_connection_id) REFERENCES oauth_connections(id)
);

CREATE TABLE IF NOT EXISTS calendar_events (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    ical_uid TEXT,
    meeting_id TEXT,
    title TEXT NOT NULL,
    meeting_url TEXT,
    starts_at TEXT,
    ends_at TEXT,
    organizer_email TEXT,
    status TEXT NOT NULL DEFAULT 'confirmed',
    raw_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (calendar_id, provider_event_id),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id),
    FOREIGN KEY (calendar_id) REFERENCES calendar_calendars(id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id)
);

CREATE TABLE IF NOT EXISTS calendar_attendees (
    id TEXT PRIMARY KEY,
    calendar_event_id TEXT NOT NULL,
    email TEXT NOT NULL,
    display_name TEXT,
    response_status TEXT,
    is_organizer INTEGER NOT NULL DEFAULT 0,
    is_self INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (calendar_event_id) REFERENCES calendar_events(id)
);

CREATE TABLE IF NOT EXISTS integration_connections (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    config_json_encrypted TEXT,
    status TEXT NOT NULL DEFAULT 'disconnected',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
);

CREATE TABLE IF NOT EXISTS integration_deliveries (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    integration_connection_id TEXT NOT NULL,
    delivery_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    provider_object_id TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    delivered_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (integration_connection_id) REFERENCES integration_connections(id)
);

CREATE TABLE IF NOT EXISTS share_recipients (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    participant_id TEXT,
    email TEXT,
    display_name TEXT,
    source TEXT NOT NULL DEFAULT 'manual',
    is_selected INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id),
    FOREIGN KEY (participant_id) REFERENCES participants(id)
);

CREATE TABLE IF NOT EXISTS email_deliveries (
    id TEXT PRIMARY KEY,
    meeting_id TEXT NOT NULL,
    recipient_email TEXT NOT NULL,
    provider TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    provider_message_id TEXT,
    error_message TEXT,
    sent_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id)
);

CREATE TABLE IF NOT EXISTS usage_daily (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    usage_date TEXT NOT NULL,
    meetings_count INTEGER NOT NULL DEFAULT 0,
    recording_minutes INTEGER NOT NULL DEFAULT 0,
    transcription_minutes INTEGER NOT NULL DEFAULT 0,
    storage_bytes INTEGER NOT NULL DEFAULT 0,
    emails_sent INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (workspace_id, usage_date),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
);

CREATE TABLE IF NOT EXISTS billing_accounts (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL UNIQUE,
    stripe_customer_id TEXT,
    stripe_subscription_id TEXT,
    plan_tier TEXT NOT NULL DEFAULT 'free',
    status TEXT NOT NULL DEFAULT 'inactive',
    current_period_start TEXT,
    current_period_end TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_members_user_id ON workspace_members(user_id);
CREATE INDEX IF NOT EXISTS idx_meetings_workspace_id ON meetings(workspace_id);
CREATE INDEX IF NOT EXISTS idx_meetings_created_by_user_id ON meetings(created_by_user_id);
CREATE INDEX IF NOT EXISTS idx_meeting_access_user_id ON meeting_access(user_id);
CREATE INDEX IF NOT EXISTS idx_recall_bots_meeting_id ON recall_bots(meeting_id);
CREATE INDEX IF NOT EXISTS idx_recordings_meeting_id ON recordings(meeting_id);
CREATE INDEX IF NOT EXISTS idx_recording_assets_recording_id ON recording_assets(recording_id);
CREATE INDEX IF NOT EXISTS idx_participants_meeting_id ON participants(meeting_id);
CREATE INDEX IF NOT EXISTS idx_participant_events_meeting_id ON participant_events(meeting_id);
CREATE INDEX IF NOT EXISTS idx_transcriptions_meeting_id ON transcriptions(meeting_id);
CREATE INDEX IF NOT EXISTS idx_transcript_segments_transcription_id ON transcript_segments(transcription_id);
CREATE INDEX IF NOT EXISTS idx_notes_meeting_id ON notes(meeting_id);
CREATE INDEX IF NOT EXISTS idx_action_items_meeting_id ON action_items(meeting_id);
CREATE INDEX IF NOT EXISTS idx_provider_events_status ON provider_events(processing_status, received_at);
CREATE INDEX IF NOT EXISTS idx_jobs_status_run_after ON jobs(status, run_after);
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_type_dedupe_active ON jobs(job_type, dedupe_key) WHERE dedupe_key IS NOT NULL AND status IN ('queued', 'retryable', 'leased');
CREATE INDEX IF NOT EXISTS idx_calendar_events_workspace_id ON calendar_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_integration_deliveries_meeting_id ON integration_deliveries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_share_recipients_meeting_id ON share_recipients(meeting_id);
CREATE INDEX IF NOT EXISTS idx_email_deliveries_meeting_id ON email_deliveries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_usage_daily_workspace_id ON usage_daily(workspace_id, usage_date);
"#;
