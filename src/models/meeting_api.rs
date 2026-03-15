use serde::{Deserialize, Serialize};

use super::MeetingDetail;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMeetingRequest {
    pub meeting_url: String,
    pub title: Option<String>,
    pub join_at: Option<String>,
    pub bot_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingsListQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMutationResponse {
    pub meeting: MeetingDetail,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingActionResponse {
    pub meeting_id: String,
    pub status: String,
    pub processing_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWebhookAck {
    pub accepted: bool,
    pub provider_event_id: String,
    pub event_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentUserResponse {
    pub user_id: String,
    pub workspace_id: String,
}
