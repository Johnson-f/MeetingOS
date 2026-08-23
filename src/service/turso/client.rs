use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use libsql::{Builder, Connection, Database};
use tracing::info;

use super::schema::migrate;

#[derive(Clone)]
pub struct TursoClient {
    db: Arc<Database>,
}

impl TursoClient {
    pub async fn connect(url: String, token: String) -> Result<Self> {
        let db = Builder::new_remote(url, token)
            .build()
            .await
            .context("failed to connect to database")?;

        let conn = db
            .connect()
            .context("failed to get database connection for migration")?;

        conn.execute("PRAGMA foreign_keys = ON", libsql::params![])
            .await
            .context("failed to enable foreign keys")?;
        migrate(&conn).await.context("schema migration failed")?;

        info!("database connection established and schema migrated");

        Ok(Self { db: Arc::new(db) })
    }

    pub async fn connection(&self) -> Result<Connection> {
        let conn = self
            .db
            .connect()
            .context("failed to get database connection")?;

        conn.execute("PRAGMA foreign_keys = ON", libsql::params![])
            .await
            .context("failed to enable foreign keys")?;

        Ok(conn)
    }
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
