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
    pub account_id: Option<String>,
    pub bucket: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub poll_interval_ms: u64,
    pub lease_seconds: i64,
    pub max_attempts: i64,
}

#[derive(Debug, Clone)]
pub struct RedisConfig {
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JinaConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub embedding_model: String,
    pub reranker_model: String,
}

#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub collection_name: String,
}

#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub role: AppRole,
    pub host: IpAddr,
    pub port: u16,
    pub environment: String,
    pub public_app_url: Option<String>,
    pub cors_allowed_origins: Vec<String>,
    pub turso_database_url: String,
    pub turso_auth_token: String,
    pub clerk_secret_key: String,
    pub recall_ai: RecallAiConfig,
    pub groq: GroqConfig,
    pub storage: StorageConfig,
    pub worker: WorkerConfig,
    pub redis: RedisConfig,
    pub jina: JinaConfig,
    pub qdrant: QdrantConfig,
    pub google_oauth: GoogleOAuthConfig,
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
            cors_allowed_origins: cors_allowed_origins_from_env(),
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
                account_id: env::var("R2_ACCOUNT_ID").ok(),
                bucket: env::var("R2_BUCKET").ok(),
                access_key_id: env::var("R2_ACCESS_KEY_ID").ok(),
                secret_access_key: env::var("R2_SECRET_ACCESS_KEY").ok(),
                endpoint: env::var("R2_ENDPOINT").ok(),
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
            redis: RedisConfig {
                url: env::var("REDIS_URL").ok(),
            },
            jina: JinaConfig {
                api_key: env::var("JINA_API_KEY").ok(),
                base_url: env::var("JINA_BASE_URL")
                    .unwrap_or_else(|_| "https://api.jina.ai".to_owned()),
                embedding_model: env::var("JINA_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| "jina-embeddings-v3".to_owned()),
                reranker_model: env::var("JINA_RERANKER_MODEL")
                    .unwrap_or_else(|_| "jina-reranker-v3".to_owned()),
            },
            qdrant: QdrantConfig {
                url: env::var("QDRANT_URL").ok(),
                api_key: env::var("QDRANT_API_KEY").ok(),
                collection_name: env::var("QDRANT_COLLECTION")
                    .unwrap_or_else(|_| "meeting_transcripts".to_owned()),
            },
            google_oauth: GoogleOAuthConfig {
                client_id: env::var("GOOGLE_CLIENT_ID").ok(),
                client_secret: env::var("GOOGLE_CLIENT_SECRET").ok(),
                redirect_uri: env::var("GOOGLE_REDIRECT_URI")
                    .unwrap_or_else(|_| "http://localhost:8080/api/v1/calendar/google/callback".to_owned()),
            },
        }
    }

    pub fn socket_address(&self) -> SocketAddr {
        SocketAddr::from((self.host, self.port))
    }
}

fn cors_allowed_origins_from_env() -> Vec<String> {
    let from_env = env::var("CORS_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !from_env.is_empty() {
        return from_env;
    }

    let mut origins = vec![
        "http://localhost:3000".to_owned(),
        "http://127.0.0.1:3000".to_owned(),
        "http://localhost:3001".to_owned(),
        "http://127.0.0.1:3001".to_owned(),
    ];

    if let Ok(ngrok_url) = env::var("NGROK_URL") {
        let trimmed = ngrok_url.trim().trim_end_matches('/').to_owned();
        if !trimmed.is_empty() {
            origins.push(trimmed);
        }
    }

    origins
}
