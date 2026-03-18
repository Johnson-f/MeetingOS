use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct RecallCreateBotRequest<'a> {
    pub meeting_url: &'a str,
    pub bot_name: &'a str,
    pub join_at: &'a str,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct RecallCreatedBot {
    pub recall_bot_id: String,
    pub status: String,
    pub raw_json: Value,
}

#[derive(Debug, Clone, Default)]
pub struct RecallRecordingMedia {
    pub recording_id: Option<String>,
    pub duration_seconds: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub audio_download_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GroqTranscriptionResponse {
    pub text: String,
    pub language: Option<String>,
    pub segments: Vec<GroqSegment>,
    pub raw_json: String,
}

#[derive(Debug, Clone)]
pub struct GroqSegment {
    pub seq: i64,
    pub speaker_label: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub confidence_json: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedActionItem {
    pub description: String,
    pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    pub due_date: Option<String>,
    pub priority: Option<String>,
    #[serde(default = "default_action_item_status")]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedNote {
    pub title: Option<String>,
    pub summary_markdown: Option<String>,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub action_items: Vec<GeneratedActionItem>,
}

fn default_action_item_status() -> String {
    "open".to_owned()
}

#[derive(Debug, Clone)]
pub struct RecallParticipant {
    pub id: String,
    pub name: String,
    pub is_host: bool,
    pub events: Vec<RecallParticipantEvent>,
}

#[derive(Debug, Clone)]
pub struct RecallParticipantEvent {
    pub event_type: String,
    pub timestamp: Option<String>,
    pub relative_ms: Option<i64>,
}
