use std::convert::Infallible;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::info;

use super::state::AppState;

pub async fn sse_events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    info!("SSE client connected");

    let rx = state.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(event) => {
                let mut data = serde_json::json!({
                    "type": event.event_type,
                    "meeting_id": event.meeting_id,
                });
                if let Some(extra) = event.data {
                    if let Some(obj) = data.as_object_mut() {
                        if let Some(extra_obj) = extra.as_object() {
                            obj.extend(extra_obj.iter().map(|(k, v)| (k.clone(), v.clone())));
                        }
                    }
                }
                Some(Ok(Event::default().data(data.to_string())))
            }
            Err(_) => None, // lagged, skip
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
