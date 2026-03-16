use tokio::sync::broadcast;

use crate::{config::AppConfig, service::ServiceRegistry};

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub meeting_id: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub services: ServiceRegistry,
    pub sse_tx: broadcast::Sender<SseEvent>,
}

impl AppState {
    pub fn new(config: AppConfig, services: ServiceRegistry) -> Self {
        let sse_tx = services.sse_tx.clone();
        Self {
            config,
            services,
            sse_tx,
        }
    }
}
