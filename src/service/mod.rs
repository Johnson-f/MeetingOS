pub mod auth;
pub mod google_calendar;
pub mod jina;
pub mod jobs;
pub mod qdrant_search;
pub mod recall_ai;
pub mod redis;
pub mod storage;
pub mod turso;
pub mod vector;

use tokio::sync::broadcast;
use tracing::info;

use crate::{config::AppConfig, routes::state::SseEvent};
use google_calendar::GoogleCalendarClient;
use jina::JinaClient;
use qdrant_search::QdrantClient;
use recall_ai::RecallAiClient;
use redis::RedisClient;
use storage::StorageClient;
use turso::client::TursoClient;

#[derive(Clone)]
pub struct ServiceRegistry {
    pub turso: TursoClient,
    pub recall_ai: Option<RecallAiClient>,
    pub storage: Option<StorageClient>,
    pub redis: Option<RedisClient>,
    pub jina: Option<JinaClient>,
    pub qdrant: Option<QdrantClient>,
    pub google_calendar: Option<GoogleCalendarClient>,
    pub config: AppConfig,
    pub sse_tx: broadcast::Sender<SseEvent>,
}

impl ServiceRegistry {
    pub async fn new(config: AppConfig, turso: TursoClient, sse_tx: broadcast::Sender<SseEvent>) -> Self {
        let recall_ai = RecallAiClient::new(&config.recall_ai);
        let storage = StorageClient::new(&config.storage).await;
        let redis = RedisClient::connect(&config.redis).await;
        let jina = JinaClient::new(&config.jina);
        let qdrant = QdrantClient::connect(&config.qdrant).await;
        let google_calendar = GoogleCalendarClient::new(&config.google_oauth);

        info!(
            recall_ai = recall_ai.is_some(),
            storage = storage.is_some(),
            redis = redis.is_some(),
            jina = jina.is_some(),
            qdrant = qdrant.is_some(),
            google_calendar = google_calendar.is_some(),
            groq = config.groq.api_key.is_some(),
            "services initialized"
        );

        Self {
            turso,
            recall_ai,
            storage,
            redis,
            jina,
            qdrant,
            google_calendar,
            config,
            sse_tx,
        }
    }

    pub fn recall_ai_ready(&self) -> bool {
        self.recall_ai.is_some()
    }

    pub fn groq_ready(&self) -> bool {
        self.config.groq.api_key.is_some()
    }

    pub fn storage_ready(&self) -> bool {
        self.storage.is_some()
    }

    pub fn turso_ready(&self) -> bool {
        true
    }
}
