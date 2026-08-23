#[derive(Debug, Clone)]
pub struct UserContext {
    pub user_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone)]
pub struct MeetingDraft {
    pub title: String,
    pub source: String,
    pub original_meeting_url: String,
    pub normalized_meeting_url: String,
    pub platform: String,
    pub meeting_time_mode: String,
    pub dedup_key: String,
    pub scheduled_start_at: String,
}

#[derive(Debug, Clone)]
pub struct UpsertMeetingResult {
    pub meeting_id: String,
    pub created: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StoredRecallBot {
    pub id: String,
    pub meeting_id: String,
    pub recall_bot_id: String,
    pub status: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StoredProviderEvent {
    pub id: String,
    pub event_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone)]
pub struct StoredJob {
    pub id: String,
    pub job_type: String,
    pub payload_json: String,
    pub attempt_count: i64,
    pub max_attempts: i64,
}

#[derive(Debug, Clone, Default)]
pub struct JobQueueStats {
    pub pending_count: i64,
    pub leased_count: i64,
    pub oldest_queued_age_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct StuckJobReport {
    pub count: i64,
    pub oldest_job_id: String,
    pub oldest_job_type: String,
    pub oldest_age_seconds: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StoredRecordingAudioAsset {
    pub status: Option<String>,
    pub storage_bucket: Option<String>,
    pub storage_key: Option<String>,
    pub mime_type: Option<String>,
    pub byte_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub source_download_url_last_seen: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StoredRecordingWithAsset {
    pub id: String,
    pub meeting_id: String,
    pub audio_asset: Option<StoredRecordingAudioAsset>,
}

#[derive(Debug, Clone)]
pub struct StoredMeetingAudioAsset {
    pub status: Option<String>,
    pub storage_bucket: Option<String>,
    pub storage_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredTranscription {
    pub id: String,
    pub full_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredTranscriptSegment {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredTranscriptionWithSegments {
    pub full_text: Option<String>,
    pub segments: Vec<StoredTranscriptSegment>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RecordingRow {
    pub id: String,
    pub meeting_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredChatThread {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredChatMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub sources_json: Option<String>,
    pub created_at: String,
}
