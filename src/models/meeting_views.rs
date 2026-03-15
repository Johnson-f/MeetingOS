use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingListItem {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub status: String,
    pub processing_status: String,
    pub scheduled_start_at: Option<String>,
    pub actual_start_at: Option<String>,
    pub actual_end_at: Option<String>,
    pub recall_bot_id: Option<String>,
    pub latest_note_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingDetail {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub source: String,
    pub original_meeting_url: String,
    pub normalized_meeting_url: String,
    pub dedup_key: String,
    pub status: String,
    pub processing_status: String,
    pub scheduled_start_at: Option<String>,
    pub scheduled_end_at: Option<String>,
    pub actual_start_at: Option<String>,
    pub actual_end_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub bot: Option<RecallBotView>,
    pub recording: Option<RecordingView>,
    pub transcription: Option<TranscriptionView>,
    pub note: Option<NoteView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallBotView {
    pub id: String,
    pub recall_bot_id: String,
    pub bot_name: String,
    pub join_at: Option<String>,
    pub status: String,
    pub status_sub_code: Option<String>,
    pub failure_detail: Option<String>,
    pub joined_at: Option<String>,
    pub left_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingView {
    pub id: String,
    pub recall_recording_id: Option<String>,
    pub status: String,
    pub duration_seconds: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub audio_asset_status: Option<String>,
    pub audio_source_download_url_last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionView {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub language: Option<String>,
    pub status: String,
    pub full_text: Option<String>,
    pub segments: Vec<TranscriptSegmentView>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentView {
    pub id: String,
    pub seq: i64,
    pub speaker_label: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteView {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub status: String,
    pub title: Option<String>,
    pub summary_markdown: Option<String>,
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub risks: Vec<String>,
    pub action_items: Vec<ActionItemView>,
    pub generated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionItemView {
    pub id: String,
    pub description: String,
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    pub due_date: Option<String>,
    pub priority: Option<String>,
    pub status: String,
    pub sort_order: i64,
}
