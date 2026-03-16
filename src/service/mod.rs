pub mod auth;
pub mod jobs;
pub mod recall_ai;
pub mod storage;
pub mod turso;

use tracing::info;

use crate::config::AppConfig;
use recall_ai::RecallAiClient;
use storage::StorageClient;
use turso::client::TursoClient;

#[derive(Clone)]
pub struct ServiceRegistry {
    pub turso: TursoClient,
    pub recall_ai: Option<RecallAiClient>,
    pub storage: Option<StorageClient>,
    pub config: AppConfig,
}

impl ServiceRegistry {
    pub async fn new(config: AppConfig, turso: TursoClient) -> Self {
        let recall_ai = RecallAiClient::new(&config.recall_ai);
        let storage = StorageClient::new(&config.storage).await;

        info!(
            recall_ai = recall_ai.is_some(),
            storage = storage.is_some(),
            groq = config.groq.api_key.is_some(),
            "services initialized"
        );

        Self {
            turso,
            recall_ai,
            storage,
            config,
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
