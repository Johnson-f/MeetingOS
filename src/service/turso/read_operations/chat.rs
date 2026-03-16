use anyhow::Result;
use libsql::params;

use crate::service::turso::client::{new_id, now_rfc3339};

use super::super::client::TursoClient;
use super::types::{StoredChatMessage, StoredChatThread};

impl TursoClient {
    pub async fn create_chat_thread(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<StoredChatThread> {
        let conn = self.connection().await?;
        let id = new_id();
        let now = now_rfc3339();

        conn.execute(
            "INSERT INTO chat_threads (id, user_id, workspace_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            params![id.as_str(), user_id, workspace_id, now.as_str(), now.as_str()],
        )
        .await?;

        Ok(StoredChatThread {
            id,
            user_id: user_id.to_string(),
            workspace_id: workspace_id.to_string(),
            title: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub async fn update_chat_thread_title(
        &self,
        thread_id: &str,
        user_id: &str,
        title: &str,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        let changed = conn
            .execute(
                "UPDATE chat_threads SET title = ?, updated_at = ? WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
                params![title, now.as_str(), thread_id, user_id],
            )
            .await?;

        Ok(changed > 0)
    }

    pub async fn list_chat_threads(
        &self,
        user_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<StoredChatThread>> {
        let conn = self.connection().await?;
        let limit = limit.unwrap_or(50);

        let mut rows = conn
            .query(
                "SELECT id, user_id, workspace_id, title, created_at, updated_at FROM chat_threads WHERE user_id = ? AND deleted_at IS NULL ORDER BY updated_at DESC LIMIT ?",
                params![user_id, limit],
            )
            .await?;

        let mut threads = Vec::new();
        while let Some(row) = rows.next().await? {
            threads.push(StoredChatThread {
                id: row.get::<String>(0)?,
                user_id: row.get::<String>(1)?,
                workspace_id: row.get::<String>(2)?,
                title: row.get::<Option<String>>(3)?,
                created_at: row.get::<String>(4)?,
                updated_at: row.get::<String>(5)?,
            });
        }

        Ok(threads)
    }

    pub async fn get_chat_thread(
        &self,
        thread_id: &str,
        user_id: &str,
    ) -> Result<Option<StoredChatThread>> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                "SELECT id, user_id, workspace_id, title, created_at, updated_at FROM chat_threads WHERE id = ? AND user_id = ? AND deleted_at IS NULL LIMIT 1",
                params![thread_id, user_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            return Ok(Some(StoredChatThread {
                id: row.get::<String>(0)?,
                user_id: row.get::<String>(1)?,
                workspace_id: row.get::<String>(2)?,
                title: row.get::<Option<String>>(3)?,
                created_at: row.get::<String>(4)?,
                updated_at: row.get::<String>(5)?,
            }));
        }

        Ok(None)
    }

    pub async fn soft_delete_chat_thread(
        &self,
        thread_id: &str,
        user_id: &str,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let now = now_rfc3339();

        let changed = conn
            .execute(
                "UPDATE chat_threads SET deleted_at = ?, updated_at = ? WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
                params![now.as_str(), now.as_str(), thread_id, user_id],
            )
            .await?;

        Ok(changed > 0)
    }

    pub async fn insert_chat_message(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        sources_json: Option<&str>,
    ) -> Result<StoredChatMessage> {
        let conn = self.connection().await?;
        let id = new_id();
        let now = now_rfc3339();

        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, role, content, sources_json, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![id.as_str(), thread_id, role, content, sources_json, now.as_str()],
        )
        .await?;

        // Touch chat_threads.updated_at
        conn.execute(
            "UPDATE chat_threads SET updated_at = ? WHERE id = ?",
            params![now.as_str(), thread_id],
        )
        .await?;

        Ok(StoredChatMessage {
            id,
            thread_id: thread_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            sources_json: sources_json.map(|s| s.to_string()),
            created_at: now,
        })
    }

    pub async fn get_chat_messages(
        &self,
        thread_id: &str,
        user_id: &str,
        limit: i64,
        before_id: Option<&str>,
    ) -> Result<Vec<StoredChatMessage>> {
        let conn = self.connection().await?;

        // Verify ownership via join to chat_threads
        let mut messages = Vec::new();

        let mut rows = if let Some(before_id) = before_id {
            conn.query(
                r#"
                SELECT cm.id, cm.thread_id, cm.role, cm.content, cm.sources_json, cm.created_at
                FROM chat_messages cm
                INNER JOIN chat_threads ct ON ct.id = cm.thread_id
                WHERE cm.thread_id = ?
                  AND ct.user_id = ?
                  AND ct.deleted_at IS NULL
                  AND cm.created_at < (SELECT created_at FROM chat_messages WHERE id = ?)
                ORDER BY cm.created_at DESC
                LIMIT ?
                "#,
                params![thread_id, user_id, before_id, limit],
            )
            .await?
        } else {
            conn.query(
                r#"
                SELECT cm.id, cm.thread_id, cm.role, cm.content, cm.sources_json, cm.created_at
                FROM chat_messages cm
                INNER JOIN chat_threads ct ON ct.id = cm.thread_id
                WHERE cm.thread_id = ?
                  AND ct.user_id = ?
                  AND ct.deleted_at IS NULL
                ORDER BY cm.created_at DESC
                LIMIT ?
                "#,
                params![thread_id, user_id, limit],
            )
            .await?
        };

        while let Some(row) = rows.next().await? {
            messages.push(StoredChatMessage {
                id: row.get::<String>(0)?,
                thread_id: row.get::<String>(1)?,
                role: row.get::<String>(2)?,
                content: row.get::<String>(3)?,
                sources_json: row.get::<Option<String>>(4)?,
                created_at: row.get::<String>(5)?,
            });
        }

        // Reverse to return in chronological order
        messages.reverse();

        Ok(messages)
    }

    pub async fn get_recent_thread_messages(
        &self,
        thread_id: &str,
        limit: i64,
    ) -> Result<Vec<StoredChatMessage>> {
        let conn = self.connection().await?;

        let mut rows = conn
            .query(
                r#"
                SELECT id, thread_id, role, content, sources_json, created_at
                FROM chat_messages
                WHERE thread_id = ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
                params![thread_id, limit],
            )
            .await?;

        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(StoredChatMessage {
                id: row.get::<String>(0)?,
                thread_id: row.get::<String>(1)?,
                role: row.get::<String>(2)?,
                content: row.get::<String>(3)?,
                sources_json: row.get::<Option<String>>(4)?,
                created_at: row.get::<String>(5)?,
            });
        }

        // Reverse to return in chronological order
        messages.reverse();

        Ok(messages)
    }
}
