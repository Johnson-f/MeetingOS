use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRole {
    Api,
    Worker,
    All,
}

impl AppRole {
    fn from_env(value: Option<String>) -> Self {
        match value
            .unwrap_or_else(|| "all".to_owned())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "api" => Self::Api,
            "worker" => Self::Worker,
            _ => Self::All,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RecallAiConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub webhook_secret: Option<String>,
    pub default_bot_name: String,
}

#[derive(Debug, Clone)]
pub struct GroqConfig {
    pub api_key: Option<String>,
    pub api_base_url: String,
    pub transcription_model: String,
    pub notes_model: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub bucket: Option<String>,
    pub public_base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub poll_interval_ms: u64,
    pub lease_seconds: i64,
    pub max_attempts: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub role: AppRole,
    pub host: IpAddr,
    pub port: u16,
    pub environment: String,
    pub public_app_url: Option<String>,
    pub turso_database_url: String,
    pub turso_auth_token: String,
    pub clerk_secret_key: String,
    pub recall_ai: RecallAiConfig,
    pub groq: GroqConfig,
    pub storage: StorageConfig,
    pub worker: WorkerConfig,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            role: AppRole::from_env(env::var("APP_ROLE").ok()),
            host: env::var("APP_HOST")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            port: env::var("PORT")
                .ok()
                .or_else(|| env::var("APP_PORT").ok())
                .and_then(|value| value.parse().ok())
                .unwrap_or(3000),
            environment: env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned()),
            public_app_url: env::var("APP_PUBLIC_URL").ok(),
            turso_database_url: env::var("TURSO_DATABASE_URL")
                .expect("TURSO_DATABASE_URL must be set"),
            turso_auth_token: env::var("TURSO_AUTH_TOKEN").expect("TURSO_AUTH_TOKEN must be set"),
            clerk_secret_key: env::var("CLERK_SECRET_KEY").expect("CLERK_SECRET_KEY must be set"),
            recall_ai: RecallAiConfig {
                base_url: env::var("RECALL_AI_BASE_URL")
                    .unwrap_or_else(|_| "https://us-west-2.recall.ai".to_owned()),
                api_key: env::var("RECALL_AI_API_KEY").ok(),
                webhook_secret: env::var("RECALL_AI_WEBHOOK_SECRET").ok(),
                default_bot_name: env::var("RECALL_AI_DEFAULT_BOT_NAME")
                    .unwrap_or_else(|_| "Meeting Bot".to_owned()),
            },
            groq: GroqConfig {
                api_key: env::var("GROQ_API_KEY").ok(),
                api_base_url: env::var("GROQ_API_BASE_URL")
                    .unwrap_or_else(|_| "https://api.groq.com".to_owned()),
                transcription_model: env::var("GROQ_TRANSCRIPTION_MODEL")
                    .unwrap_or_else(|_| "whisper-large-v3".to_owned()),
                notes_model: env::var("GROQ_NOTES_MODEL")
                    .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_owned()),
            },
            storage: StorageConfig {
                bucket: env::var("R2_BUCKET").ok(),
                public_base_url: env::var("R2_PUBLIC_BASE_URL").ok(),
            },
            worker: WorkerConfig {
                poll_interval_ms: env::var("WORKER_POLL_INTERVAL_MS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(2_000),
                lease_seconds: env::var("WORKER_LEASE_SECONDS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(60),
                max_attempts: env::var("WORKER_MAX_ATTEMPTS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(8),
            },
        }
    }

    pub fn socket_address(&self) -> SocketAddr {
        SocketAddr::from((self.host, self.port))
    }
}
