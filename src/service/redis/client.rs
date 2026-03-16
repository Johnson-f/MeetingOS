use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tracing::{info, warn};

use crate::config::RedisConfig;

#[derive(Clone)]
pub struct RedisClient {
    conn: ConnectionManager,
}

impl RedisClient {
    pub async fn connect(config: &RedisConfig) -> Option<Self> {
        let url = config.url.as_deref()?;

        let client = match redis::Client::open(url) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "failed to create Redis client");
                return None;
            }
        };

        match ConnectionManager::new(client).await {
            Ok(conn) => {
                // Verify connection with a PING
                let mut test_conn = conn.clone();
                match redis::cmd("PING")
                    .query_async::<String>(&mut test_conn)
                    .await
                {
                    Ok(_) => info!("Redis connection established"),
                    Err(e) => {
                        warn!(error = %e, "Redis PING failed");
                        return None;
                    }
                }
                Some(Self { conn })
            }
            Err(e) => {
                warn!(error = %e, "failed to connect to Redis");
                None
            }
        }
    }

    pub async fn set(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(key, value, ttl_seconds)
            .await
            .context("redis SET failed")?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.conn.clone();
        let value: Option<String> = conn.get(key).await.context("redis GET failed")?;
        Ok(value)
    }

    pub async fn del(&self, key: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        conn.del::<_, ()>(key).await.context("redis DEL failed")?;
        Ok(())
    }

    pub async fn set_json<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: u64,
    ) -> Result<()> {
        let json = serde_json::to_string(value).context("failed to serialize to JSON")?;
        self.set(key, &json, ttl_seconds).await
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.get(key).await? {
            Some(json) => {
                let value =
                    serde_json::from_str(&json).context("failed to deserialize JSON from Redis")?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    pub async fn exists(&self, key: &str) -> Result<bool> {
        let mut conn = self.conn.clone();
        let exists: bool = conn.exists(key).await.context("redis EXISTS failed")?;
        Ok(exists)
    }
}
