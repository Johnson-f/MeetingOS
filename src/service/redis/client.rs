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
}
