use axum::http::{HeaderMap, StatusCode};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde_json::Value;

use crate::{
    config::AppConfig,
    models::{ApiError, ApiInfo},
    service::turso::read_operations::UserContext,
};

use super::state::AppState;

pub(crate) async fn current_user(
    state: &AppState,
    jwt: &ClerkJwt,
) -> Result<UserContext, ApiError> {
    state
        .services
        .turso
        .ensure_user_workspace(&jwt.sub)
        .await
        .map_err(ApiError::from)
}

pub(crate) fn api_info(config: &AppConfig) -> ApiInfo {
    ApiInfo {
        name: "meeting-bot",
        environment: config.environment.clone(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub(crate) fn extract_subject_id(payload: &Value) -> Option<String> {
    [
        payload.pointer("/data/bot/id"),
        payload.pointer("/data/recording/id"),
        payload.pointer("/data/bot_id"),
        payload.pointer("/data/recording_id"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .map(str::to_owned)
}

pub(crate) fn header_string(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok().map(str::to_owned))
}

pub(crate) fn recall_unavailable_error() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "Recall.ai is not configured",
    )
}
