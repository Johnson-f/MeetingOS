use anyhow::Result;
use libsql::params;

use crate::service::turso::{
    client::{new_id, now_rfc3339},
    read_operations::{UserContext, helpers::query_optional_string},
};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn ensure_user_workspace(&self, clerk_user_id: &str) -> Result<UserContext> {
        let conn = self.connection().await?;

        if let Some(user_id) = query_optional_string(
            &conn,
            "SELECT id FROM users WHERE clerk_user_id = ? LIMIT 1",
            params![clerk_user_id],
        )
        .await?
        {
            if let Some(workspace_id) = query_optional_string(
                &conn,
                "SELECT workspace_id FROM workspace_members WHERE user_id = ? ORDER BY created_at ASC LIMIT 1",
                params![user_id.as_str()],
            )
            .await?
            {
                return Ok(UserContext {
                    user_id,
                    workspace_id,
                });
            }
        }

        let user_id = new_id();
        let workspace_id = new_id();
        let created_at = now_rfc3339();
        let slug = format!("workspace-{}", &user_id[..8]);

        conn.execute(
            "INSERT INTO users (id, clerk_user_id, created_at, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT (clerk_user_id) DO NOTHING",
            (
                user_id.as_str(),
                clerk_user_id,
                created_at.as_str(),
                created_at.as_str(),
            ),
        )
        .await?;

        // Re-read the actual user_id in case another request won the race
        let actual_user_id = query_optional_string(
            &conn,
            "SELECT id FROM users WHERE clerk_user_id = ? LIMIT 1",
            params![clerk_user_id],
        )
        .await?
        .unwrap_or(user_id);

        // Check if workspace already exists for this user (race loser path)
        if let Some(workspace_id) = query_optional_string(
            &conn,
            "SELECT workspace_id FROM workspace_members WHERE user_id = ? ORDER BY created_at ASC LIMIT 1",
            params![actual_user_id.as_str()],
        )
        .await?
        {
            return Ok(UserContext {
                user_id: actual_user_id,
                workspace_id,
            });
        }

        conn.execute(
            "INSERT INTO workspaces (id, slug, name, owner_user_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            (
                workspace_id.as_str(),
                slug.as_str(),
                format!("{} workspace", &actual_user_id[..8]),
                actual_user_id.as_str(),
                created_at.as_str(),
                created_at.as_str(),
            ),
        )
        .await?;
        conn.execute(
            "INSERT INTO workspace_members (id, workspace_id, user_id, created_at) VALUES (?, ?, ?, ?)",
            (new_id(), workspace_id.as_str(), actual_user_id.as_str(), created_at.as_str()),
        )
        .await?;

        Ok(UserContext {
            user_id: actual_user_id,
            workspace_id,
        })
    }
}
