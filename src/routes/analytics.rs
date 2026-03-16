use axum::{
    Json,
    extract::{Extension, State},
};
use clerk_rs::validators::authorizer::ClerkJwt;

use tracing::info;

use crate::{
    models::{AnalyticsOverview, ApiError},
    routes::{helpers::current_user, state::AppState},
};

pub async fn analytics_overview(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
) -> Result<Json<serde_json::Value>, ApiError> {
    info!(sub = %jwt.sub, "GET /api/v1/analytics/overview");
    let user = current_user(&state, &jwt).await?;

    // Try Redis cache first
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached)) = redis.get_cached_analytics(&user.user_id).await {
            info!(sub = %jwt.sub, "cache hit: analytics");
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&cached) {
                return Ok(Json(value));
            }
        }
    }

    let analytics = state
        .services
        .turso
        .get_analytics_overview_for_user(&user.user_id)
        .await?;

    // Write to cache
    if let Some(redis) = &state.services.redis {
        if let Ok(json) = serde_json::to_string(&analytics) {
            let _ = redis.set_cached_analytics(&user.user_id, &json).await;
        }
    }

    Ok(Json(serde_json::to_value(analytics).unwrap_or_default()))
}
