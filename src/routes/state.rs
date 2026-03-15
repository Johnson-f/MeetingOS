use crate::{config::AppConfig, service::ServiceRegistry};

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub services: ServiceRegistry,
}

impl AppState {
    pub fn new(config: AppConfig, services: ServiceRegistry) -> Self {
        Self { config, services }
    }
}
