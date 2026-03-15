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
    pub dedup_key: String,
    pub scheduled_start_at: Option<String>,
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

#[derive(Debug, Clone)]
pub struct StoredRecordingAudioAsset {
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
pub struct StoredTranscription {
    pub id: String,
    pub full_text: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RecordingRow {
    pub id: String,
    pub meeting_id: String,
}
