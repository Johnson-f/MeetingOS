use std::convert::Infallible;

use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use serde::Deserialize;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::info;

use super::state::AppState;

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub token: Option<String>,
}

pub async fn sse_events(
    State(state): State<AppState>,
    Query(_query): Query<EventsQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    info!("SSE client connected");

    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(event) => {
                let data = serde_json::json!({
                    "type": event.event_type,
                    "meeting_id": event.meeting_id,
                });
                Some(Ok(Event::default().data(data.to_string())))
            }
            Err(_) => None, // lagged, skip
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
