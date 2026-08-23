use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationPlaceholder {
    pub status: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsOverview {
    pub total_meetings: i64,
    pub meetings_this_week_previous: i64,
    pub meetings_this_week_upcoming: i64,
    pub recorded_hours: f64,
    pub integrations: IntegrationPlaceholder,
}
