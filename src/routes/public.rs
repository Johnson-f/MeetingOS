use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

use crate::models::{ApiError, HealthResponse, ServiceStatusResponse};

use super::{helpers::api_info, state::AppState};

pub async fn root() -> Json<Value> {
    Json(json!({
        "name": "meeting-bot",
        "message": "Meeting bot API is running",
        "routes": ["/health", "/api/v1/status"],
    }))
}

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        info: api_info(&state.config),
    })
}

pub async fn status(State(state): State<AppState>) -> Json<ServiceStatusResponse> {
    Json(ServiceStatusResponse {
        api: api_info(&state.config),
        bind_address: state.config.socket_address().to_string(),
        role: format!("{:?}", state.config.role).to_lowercase(),
        recall_ai_ready: state.services.recall_ai_ready(),
        groq_ready: state.services.groq_ready(),
        storage_ready: state.services.storage_ready(),
        turso_ready: state.services.turso_ready(),
    })
}

pub async fn not_implemented() -> Result<StatusCode, ApiError> {
    Err(ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "this integration endpoint is scaffolded but not implemented yet",
    ))
}
