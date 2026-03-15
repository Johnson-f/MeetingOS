pub mod auth;
pub mod jobs;
pub mod recall_ai;
pub mod turso;

use crate::config::AppConfig;
use recall_ai::RecallAiClient;
use turso::client::TursoClient;

#[derive(Clone)]
pub struct ServiceRegistry {
    pub turso: TursoClient,
    pub recall_ai: Option<RecallAiClient>,
    pub config: AppConfig,
}

impl ServiceRegistry {
    pub fn new(config: AppConfig, turso: TursoClient) -> Self {
        let recall_ai = RecallAiClient::new(&config.recall_ai);

        Self {
            turso,
            recall_ai,
            config,
        }
    }

    pub fn recall_ai_ready(&self) -> bool {
        self.recall_ai.is_some()
    }

    pub fn groq_ready(&self) -> bool {
        self.config.groq.api_key.is_some()
    }

    pub fn turso_ready(&self) -> bool {
        true
    }
}
