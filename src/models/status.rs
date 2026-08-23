use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiInfo {
    pub name: &'static str,
    pub environment: String,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub info: ApiInfo,
}

#[derive(Debug, Serialize)]
pub struct ServiceStatusResponse {
    pub api: ApiInfo,
    pub bind_address: String,
    pub role: String,
    pub recall_ai_ready: bool,
    pub groq_ready: bool,
    pub storage_ready: bool,
    pub turso_ready: bool,
}
