use axum::{
    Router,
    http::{HeaderValue, Method, header},
    routing::{get, patch, post},
};
use clerk_rs::validators::axum::ClerkLayer;
use clerk_rs::validators::jwks::MemoryCacheJwksProvider;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

use super::{
    analytics::analytics_overview,
    calendar::{google_callback, google_connect, google_disconnect, google_resync, google_status},
    chat::{chat_stream, delete_thread, get_thread_messages, list_threads, update_thread},
    events::sse_events,
    meetings::{
        cancel_meeting, create_meeting, delete_meeting, get_audio, get_meeting, get_note,
        list_meetings, me, update_meeting,
    },
    public::{health, not_implemented, root, status},
    sharing::{get_shared_meeting, list_participants, share_meeting},
    state::AppState,
    webhooks::{google_calendar_webhook, recall_webhook},
};

pub fn app_router(state: AppState, jwks_provider: MemoryCacheJwksProvider) -> Router {
    let cors = build_cors_layer(&state);
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/webhooks/recall", post(recall_webhook))
        .route("/api/v1/events", get(sse_events))
        .route(
            "/api/v1/webhooks/google-calendar",
            post(google_calendar_webhook),
        )
        .route("/api/v1/calendar/google/callback", get(google_callback))
        .route("/api/v1/webhooks/microsoft-graph", post(not_implemented))
        .route("/api/v1/share/{token}", get(get_shared_meeting));

    let protected_routes = Router::new()
        .route("/api/v1/me", get(me))
        .route("/api/v1/analytics/overview", get(analytics_overview))
        .route("/api/v1/meetings", post(create_meeting).get(list_meetings))
        .route("/api/v1/chat", post(chat_stream))
        .route("/api/v1/chat/threads", get(list_threads))
        .route(
            "/api/v1/chat/threads/{id}",
            patch(update_thread).delete(delete_thread),
        )
        .route(
            "/api/v1/chat/threads/{id}/messages",
            get(get_thread_messages),
        )
        .route(
            "/api/v1/meetings/{meeting_id}",
            get(get_meeting)
                .patch(update_meeting)
                .delete(delete_meeting),
        )
        .route("/api/v1/meetings/{meeting_id}/cancel", post(cancel_meeting))
        .route("/api/v1/meetings/{meeting_id}/audio", get(get_audio))
        .route("/api/v1/meetings/{meeting_id}/share", post(share_meeting))
        .route(
            "/api/v1/meetings/{meeting_id}/participants",
            get(list_participants),
        )
        .route("/api/v1/notes/{meeting_id}", get(get_note))
        .route("/api/v1/calendar/google/connect", get(google_connect))
        .route(
            "/api/v1/calendar/google/disconnect",
            post(google_disconnect),
        )
        .route("/api/v1/calendar/google/resync", post(google_resync))
        .route("/api/v1/calendar/google/status", get(google_status))
        .route("/api/v1/calendar/microsoft/connect", post(not_implemented))
        .route("/api/v1/calendar/microsoft/callback", get(not_implemented))
        .route(
            "/api/v1/calendar/microsoft/disconnect",
            post(not_implemented),
        )
        .route("/api/v1/calendar/microsoft/resync", post(not_implemented))
        .route_layer(ClerkLayer::new(jwks_provider, None, true));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}

fn build_cors_layer(state: &AppState) -> CorsLayer {
    let origins = state
        .config
        .cors_allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
}
