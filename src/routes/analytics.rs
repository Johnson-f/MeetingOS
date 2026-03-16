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
) -> Result<Json<AnalyticsOverview>, ApiError> {
    info!(sub = %jwt.sub, "GET /api/v1/analytics/overview");
    let user = current_user(&state, &jwt).await?;
    let analytics = state
        .services
        .turso
        .get_analytics_overview_for_user(&user.user_id)
        .await?;

    Ok(Json(analytics))
}
