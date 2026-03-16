# Chat Persistence Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add persistent chat threads with message history, AI-generated titles, and cross-thread vector memory to the meeting-bot.

**Architecture:** Two new DB tables (`chat_threads`, `chat_messages`), refactored Qdrant schema to support polymorphic source types, new REST endpoints for thread CRUD, modified chat streaming endpoint to persist messages and enqueue vectorization jobs, and a reworked frontend chat panel with thread list/conversation views.

**Tech Stack:** Rust/Axum, Turso (libsql), Qdrant, Jina embeddings, Groq LLM, Next.js/React

**Spec:** `docs/superpowers/specs/2026-03-16-chat-persistence-design.md`

---

## File Map

### Backend — New Files
- `src/service/turso/read_operations/chat.rs` — DB queries for threads and messages
- `src/routes/chat.rs` — Route handlers for thread CRUD + updated chat stream

### Backend — Modified Files
- `src/service/turso/schema/tables/mod.rs` — Add `chat_threads` and `chat_messages` tables
- `src/service/turso/schema/logic.rs` — Bump schema version
- `src/service/turso/read_operations/mod.rs` — Declare `chat` module, export types
- `src/service/turso/read_operations/types.rs` — Add chat-related structs
- `src/service/qdrant_search/client.rs` — Refactor `SearchResult`/`ChunkPoint` for polymorphic sources, add `source_type` index
- `src/service/vector/mod.rs` — Export new `vectorize_chat_qa` function
- `src/service/vector/search.rs` — Add `vectorize_chat_qa` function
- `src/service/jobs/constants.rs` — Add `JOB_VECTORIZE_CHAT_QA` constant
- `src/service/jobs/handlers.rs` — Add `vectorize_chat_qa_job` handler
- `src/service/jobs/runner.rs` — Register new job type
- `src/routes/mod.rs` — Declare `chat` module
- `src/routes/router.rs` — Register new routes
- `src/routes/search.rs` — Remove old `chat_stream` (moved to chat.rs)

### Frontend — Modified Files
- `frontend/src/lib/types/meetings.ts` — Add thread/message types, update `SearchSource`
- `frontend/src/lib/backend_connection/client.ts` — Add thread/message API methods
- `frontend/src/components/chat.tsx` — Rewrite with thread list, conversation view, persistence

---

## Chunk 1: Database Schema & Turso Operations

### Task 1: Add chat tables to schema

**Files:**
- Modify: `src/service/turso/schema/tables/mod.rs`
- Modify: `src/service/turso/schema/logic.rs`

- [ ] **Step 1: Add chat_threads and chat_messages tables to SCHEMA_SQL**

Append to the end of the `SCHEMA_SQL` string in `src/service/turso/schema/tables/mod.rs`:

```sql
CREATE TABLE IF NOT EXISTS chat_threads (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    title TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    FOREIGN KEY (user_id) REFERENCES users(id)
);
CREATE INDEX IF NOT EXISTS idx_chat_threads_user ON chat_threads(user_id, deleted_at);

CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    sources_json TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (thread_id) REFERENCES chat_threads(id)
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_thread ON chat_messages(thread_id, created_at);
```

- [ ] **Step 2: Bump schema version**

In `src/service/turso/schema/logic.rs`, change:
```rust
const SCHEMA_VERSION: &str = "0.3";
```

- [ ] **Step 3: Run the server to verify migration**

Run: `cargo run`
Expected: `INFO schema is up to date at v0.3` (or migration runs successfully)

- [ ] **Step 4: Commit**

```bash
git add src/service/turso/schema/tables/mod.rs src/service/turso/schema/logic.rs
git commit -m "feat: add chat_threads and chat_messages tables"
```

---

### Task 2: Add chat types to Turso

**Files:**
- Modify: `src/service/turso/read_operations/types.rs`

- [ ] **Step 1: Add ChatThread and ChatMessage structs**

Add to `src/service/turso/read_operations/types.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredChatThread {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredChatMessage {
    pub id: String,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub sources_json: Option<String>,
    pub created_at: String,
}
```

- [ ] **Step 2: Export new types from mod.rs**

In `src/service/turso/read_operations/mod.rs`, add to the `pub use types::{...}` block:
```rust
StoredChatThread, StoredChatMessage,
```

- [ ] **Step 3: Commit**

```bash
git add src/service/turso/read_operations/types.rs src/service/turso/read_operations/mod.rs
git commit -m "feat: add StoredChatThread and StoredChatMessage types"
```

---

### Task 3: Create chat DB operations

**Files:**
- Create: `src/service/turso/read_operations/chat.rs`
- Modify: `src/service/turso/read_operations/mod.rs`

- [ ] **Step 1: Create chat.rs with thread CRUD operations**

Create `src/service/turso/read_operations/chat.rs`:

```rust
use anyhow::Result;
use libsql::params;

use super::helpers::{new_id, now_rfc3339};
use super::types::{StoredChatMessage, StoredChatThread};
use crate::service::turso::TursoClient;

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
        ).await?;
        Ok(StoredChatThread {
            id,
            user_id: user_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
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
        let changed = conn.execute(
            "UPDATE chat_threads SET title = ?, updated_at = ? WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            params![title, now.as_str(), thread_id, user_id],
        ).await?;
        Ok(changed > 0)
    }

    pub async fn list_chat_threads(
        &self,
        user_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<StoredChatThread>> {
        let conn = self.connection().await?;
        let query = if let Some(limit) = limit {
            format!(
                "SELECT id, user_id, workspace_id, title, created_at, updated_at FROM chat_threads WHERE user_id = ? AND deleted_at IS NULL ORDER BY updated_at DESC LIMIT {}",
                limit
            )
        } else {
            "SELECT id, user_id, workspace_id, title, created_at, updated_at FROM chat_threads WHERE user_id = ? AND deleted_at IS NULL ORDER BY updated_at DESC".to_owned()
        };
        let mut rows = conn.query(&query, params![user_id]).await?;
        let mut threads = Vec::new();
        while let Some(row) = rows.next().await? {
            threads.push(StoredChatThread {
                id: row.get(0)?,
                user_id: row.get(1)?,
                workspace_id: row.get(2)?,
                title: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
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
        let mut rows = conn.query(
            "SELECT id, user_id, workspace_id, title, created_at, updated_at FROM chat_threads WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            params![thread_id, user_id],
        ).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(StoredChatThread {
            id: row.get(0)?,
            user_id: row.get(1)?,
            workspace_id: row.get(2)?,
            title: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        }))
    }

    pub async fn soft_delete_chat_thread(
        &self,
        thread_id: &str,
        user_id: &str,
    ) -> Result<bool> {
        let conn = self.connection().await?;
        let now = now_rfc3339();
        let changed = conn.execute(
            "UPDATE chat_threads SET deleted_at = ?, updated_at = ? WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            params![now.as_str(), now.as_str(), thread_id, user_id],
        ).await?;
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
        ).await?;
        // Touch thread updated_at
        conn.execute(
            "UPDATE chat_threads SET updated_at = ? WHERE id = ?",
            params![now.as_str(), thread_id],
        ).await?;
        Ok(StoredChatMessage {
            id,
            thread_id: thread_id.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            sources_json: sources_json.map(str::to_owned),
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
        // Verify thread ownership
        let mut check = conn.query(
            "SELECT 1 FROM chat_threads WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            params![thread_id, user_id],
        ).await?;
        if check.next().await?.is_none() {
            anyhow::bail!("thread not found or not owned by user");
        }

        let mut messages = if let Some(before_id) = before_id {
            let mut rows = conn.query(
                "SELECT id, thread_id, role, content, sources_json, created_at FROM chat_messages WHERE thread_id = ? AND created_at < (SELECT created_at FROM chat_messages WHERE id = ?) ORDER BY created_at DESC LIMIT ?",
                params![thread_id, before_id, limit],
            ).await?;
            let mut msgs = Vec::new();
            while let Some(row) = rows.next().await? {
                msgs.push(StoredChatMessage {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    sources_json: row.get(4)?,
                    created_at: row.get(5)?,
                });
            }
            msgs
        } else {
            let mut rows = conn.query(
                "SELECT id, thread_id, role, content, sources_json, created_at FROM chat_messages WHERE thread_id = ? ORDER BY created_at DESC LIMIT ?",
                params![thread_id, limit],
            ).await?;
            let mut msgs = Vec::new();
            while let Some(row) = rows.next().await? {
                msgs.push(StoredChatMessage {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    sources_json: row.get(4)?,
                    created_at: row.get(5)?,
                });
            }
            msgs
        };
        // Reverse to chronological order
        messages.reverse();
        Ok(messages)
    }

    pub async fn get_recent_thread_messages(
        &self,
        thread_id: &str,
        limit: i64,
    ) -> Result<Vec<StoredChatMessage>> {
        let conn = self.connection().await?;
        let mut rows = conn.query(
            "SELECT id, thread_id, role, content, sources_json, created_at FROM chat_messages WHERE thread_id = ? ORDER BY created_at DESC LIMIT ?",
            params![thread_id, limit],
        ).await?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next().await? {
            messages.push(StoredChatMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                sources_json: row.get(4)?,
                created_at: row.get(5)?,
            });
        }
        messages.reverse();
        Ok(messages)
    }
}
```

- [ ] **Step 2: Register chat module in mod.rs**

Add `mod chat;` to `src/service/turso/read_operations/mod.rs`.

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/service/turso/read_operations/chat.rs src/service/turso/read_operations/mod.rs
git commit -m "feat: add chat thread and message DB operations"
```

---

## Chunk 2: Qdrant Schema Refactor

### Task 4: Refactor SearchResult and ChunkPoint for polymorphic sources

**Files:**
- Modify: `src/service/qdrant_search/client.rs`

- [ ] **Step 1: Update SearchResult struct**

Replace the existing `SearchResult` struct (around line 40-48) with:

```rust
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub source_type: String,
    pub text: String,
    // Transcript-specific
    pub meeting_id: Option<String>,
    pub meeting_title: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub speaker_label: Option<String>,
    // Chat-specific
    pub thread_id: Option<String>,
    pub created_at: Option<String>,
}
```

- [ ] **Step 2: Add ChatQAPoint struct**

Add after ChunkPoint:

```rust
#[derive(Debug, Clone)]
pub struct ChatQAPoint {
    pub id: String,
    pub user_id: String,
    pub thread_id: String,
    pub text: String,
    pub created_at: String,
    pub dense_vector: Vec<f32>,
}
```

- [ ] **Step 3: Add source_type to ChunkPoint payload in upsert_chunks**

In the `upsert_chunks` function, add `"source_type"` to the payload map where points are built:

```rust
payload.insert("source_type".to_owned(), Value::from("transcript"));
```

- [ ] **Step 4: Add upsert_chat_qa_points method**

Add a new method to `QdrantSearchClient`:

```rust
pub async fn upsert_chat_qa_points(&self, points: Vec<ChatQAPoint>) -> Result<()> {
    if points.is_empty() {
        return Ok(());
    }
    let qdrant_points: Vec<PointStruct> = points
        .into_iter()
        .map(|p| {
            let mut payload = std::collections::HashMap::new();
            payload.insert("source_type".to_owned(), Value::from("chat"));
            payload.insert("user_id".to_owned(), Value::from(p.user_id));
            payload.insert("thread_id".to_owned(), Value::from(p.thread_id));
            payload.insert("text".to_owned(), Value::from(p.text));
            payload.insert("created_at".to_owned(), Value::from(p.created_at));
            PointStruct::new(
                p.id,
                Vectors::from(vec![(DENSE_VECTOR_NAME.to_owned(), p.dense_vector)]),
                Payload::from(payload),
            )
        })
        .collect();
    self.client
        .upsert_points(self.collection_name.clone(), None, qdrant_points, None)
        .await
        .context("failed to upsert chat QA points")?;
    Ok(())
}
```

- [ ] **Step 5: Update hybrid_search result parsing**

In the `hybrid_search` function, update the result parsing from the `filter_map` that drops points missing `meeting_id`/`start_ms`/`end_ms` to:

```rust
.filter_map(|point| {
    let payload = point.payload;
    let text = payload.get("text")?.as_str()?.to_owned();
    let source_type = payload
        .get("source_type")
        .and_then(|v| v.as_str())
        .unwrap_or("transcript")
        .to_owned();

    Some(SearchResult {
        source_type,
        text,
        meeting_id: payload.get("meeting_id").and_then(|v| v.as_str()).map(str::to_owned),
        meeting_title: payload.get("meeting_title").and_then(|v| v.as_str()).map(str::to_owned),
        start_ms: payload.get("start_ms").and_then(|v| v.as_integer()),
        end_ms: payload.get("end_ms").and_then(|v| v.as_integer()),
        speaker_label: payload.get("speaker_label").and_then(|v| v.as_str()).map(str::to_owned),
        thread_id: payload.get("thread_id").and_then(|v| v.as_str()).map(str::to_owned),
        created_at: payload.get("created_at").and_then(|v| v.as_str()).map(str::to_owned),
    })
})
```

- [ ] **Step 6: Add source_type keyword index in ensure_indexes**

Add to `ensure_indexes`:

```rust
self.client
    .create_field_index(
        self.collection_name.clone(),
        "source_type",
        FieldType::Keyword,
        None,
        None,
    )
    .await
    .ok();
```

- [ ] **Step 7: Fix all compilation errors from SearchResult change**

The `SearchResult` fields changed from required to optional. Update all consumers:
- `src/routes/search.rs` — where results are formatted into context and source responses
- `src/service/jina/client.rs` — if it references SearchResult (unlikely)

In `src/routes/search.rs`, the source building code needs to handle optional fields. Update the source mapping to use `unwrap_or_default()` for optional fields.

- [ ] **Step 8: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

- [ ] **Step 9: Commit**

```bash
git add src/service/qdrant_search/client.rs src/routes/search.rs
git commit -m "feat: refactor Qdrant schema for polymorphic source types"
```

---

## Chunk 3: Vectorize Chat QA Job

### Task 5: Add vectorize_chat_qa function

**Files:**
- Modify: `src/service/vector/search.rs`
- Modify: `src/service/vector/mod.rs`

- [ ] **Step 1: Add vectorize_chat_qa function**

Add to `src/service/vector/search.rs`:

```rust
pub async fn vectorize_chat_qa(
    services: &ServiceRegistry,
    thread_id: &str,
    user_id: &str,
    question: &str,
    answer: &str,
) -> Result<()> {
    use crate::service::qdrant_search::ChatQAPoint;

    // Skip if answer is too short or is an error
    if answer.len() < 50 || answer.starts_with("Error:") {
        tracing::info!(thread_id = %thread_id, "skipping chat QA vectorization: answer too short or error");
        return Ok(());
    }

    let jina = services.jina.as_ref().context("jina not configured")?;
    let qdrant = services.qdrant.as_ref().context("qdrant not configured")?;

    let text = format!("Q: {}\nA: {}", question, answer);
    let embeddings = jina.embed(vec![text.clone()], "retrieval.passage").await?;

    if embeddings.is_empty() {
        anyhow::bail!("no embedding returned for chat QA");
    }

    let point = ChatQAPoint {
        id: uuid::Uuid::new_v4().to_string(),
        user_id: user_id.to_owned(),
        thread_id: thread_id.to_owned(),
        text,
        created_at: chrono::Utc::now().to_rfc3339(),
        dense_vector: embeddings.into_iter().next().unwrap(),
    };

    qdrant.upsert_chat_qa_points(vec![point]).await?;
    tracing::info!(thread_id = %thread_id, "chat QA vectorized");
    Ok(())
}
```

- [ ] **Step 2: Export from mod.rs**

Add to `src/service/vector/mod.rs`:
```rust
pub use search::vectorize_chat_qa;
```

- [ ] **Step 3: Commit**

```bash
git add src/service/vector/search.rs src/service/vector/mod.rs
git commit -m "feat: add vectorize_chat_qa function"
```

---

### Task 6: Register vectorize_chat_qa job

**Files:**
- Modify: `src/service/jobs/constants.rs`
- Modify: `src/service/jobs/handlers.rs`
- Modify: `src/service/jobs/runner.rs`

- [ ] **Step 1: Add job constant**

Add to `src/service/jobs/constants.rs`:
```rust
pub const JOB_VECTORIZE_CHAT_QA: &str = "vectorize_chat_qa";
```

- [ ] **Step 2: Add job handler**

Add to `src/service/jobs/handlers.rs`:

```rust
pub(super) async fn vectorize_chat_qa_job(
    services: &ServiceRegistry,
    payload: &Value,
) -> Result<()> {
    let thread_id = payload.get("thread_id").and_then(Value::as_str).context("missing thread_id")?;
    let user_id = payload.get("user_id").and_then(Value::as_str).context("missing user_id")?;
    let question = payload.get("question").and_then(Value::as_str).context("missing question")?;
    let answer = payload.get("answer").and_then(Value::as_str).context("missing answer")?;

    info!(thread_id = %thread_id, "vectorizing chat Q&A");
    crate::service::vector::vectorize_chat_qa(services, thread_id, user_id, question, answer).await?;
    Ok(())
}
```

- [ ] **Step 3: Register in runner.rs**

Add `JOB_VECTORIZE_CHAT_QA` to the imports and `vectorize_chat_qa_job` to the handler imports. Add match arm:

```rust
JOB_VECTORIZE_CHAT_QA => vectorize_chat_qa_job(services, &payload).await,
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src/service/jobs/constants.rs src/service/jobs/handlers.rs src/service/jobs/runner.rs
git commit -m "feat: register vectorize_chat_qa job"
```

---

## Chunk 3: Chat Route Handlers

### Task 7: Create chat route module

**Files:**
- Create: `src/routes/chat.rs`
- Modify: `src/routes/mod.rs`
- Modify: `src/routes/router.rs`

- [ ] **Step 1: Create chat.rs with thread CRUD handlers**

Create `src/routes/chat.rs`:

```rust
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use axum_extra::sse::{Event, KeepAlive, Sse};
use clerk_rs::validators::clerk::ClerkJwt;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

use super::helpers::current_user;
use super::state::AppState;
use crate::service::ApiError;
use crate::service::jobs::constants::JOB_VECTORIZE_CHAT_QA;

#[derive(Debug, Deserialize)]
pub struct ListThreadsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateThreadBody {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct GetMessagesQuery {
    pub limit: Option<i64>,
    pub before: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub query: String,
    pub thread_id: Option<String>,
    pub meeting_id: Option<String>,
}

pub async fn list_threads(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Query(params): Query<ListThreadsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let threads = state.services.turso.list_chat_threads(&user.user_id, params.limit).await?;
    Ok(Json(json!({ "threads": threads })))
}

pub async fn get_thread_messages(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
    Query(params): Query<GetMessagesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let limit = params.limit.unwrap_or(50);
    let messages = state.services.turso.get_chat_messages(
        &thread_id,
        &user.user_id,
        limit,
        params.before.as_deref(),
    ).await?;
    Ok(Json(json!({ "messages": messages })))
}

pub async fn update_thread(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
    Json(body): Json<UpdateThreadBody>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let updated = state.services.turso
        .update_chat_thread_title(&thread_id, &user.user_id, &body.title)
        .await?;
    if !updated {
        return Err(ApiError::not_found("thread not found"));
    }
    Ok(Json(json!({ "success": true })))
}

pub async fn delete_thread(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let deleted = state.services.turso
        .soft_delete_chat_thread(&thread_id, &user.user_id)
        .await?;
    if !deleted {
        return Err(ApiError::not_found("thread not found"));
    }
    Ok(Json(json!({ "success": true })))
}
```

- [ ] **Step 2: Register module and routes**

In `src/routes/mod.rs`, add:
```rust
mod chat;
```

In `src/routes/router.rs`, add the import and routes to the protected_routes:
```rust
use super::chat::{list_threads, get_thread_messages, update_thread, delete_thread};
```

Add routes:
```rust
.route("/api/v1/chat/threads", get(list_threads))
.route("/api/v1/chat/threads/{id}", patch(update_thread).delete(delete_thread))
.route("/api/v1/chat/threads/{id}/messages", get(get_thread_messages))
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/routes/chat.rs src/routes/mod.rs src/routes/router.rs
git commit -m "feat: add chat thread CRUD endpoints"
```

---

### Task 8: Move and rewrite chat_stream handler with persistence

**Files:**
- Modify: `src/routes/chat.rs`
- Modify: `src/routes/search.rs`
- Modify: `src/routes/router.rs`

- [ ] **Step 1: Add chat_stream to chat.rs**

Add the streaming chat handler to `src/routes/chat.rs`. This is the core handler that:
- Creates thread if needed (lazy)
- Saves user message
- Loads conversation history (last 20 messages)
- Runs search pipeline (embed → Qdrant → rerank)
- Formats context with conversation history + sources
- Streams Groq response
- On completion: saves assistant message, enqueues vectorization job
- Fires title generation concurrently for new threads

The full implementation follows the existing `chat_stream` pattern in `search.rs` but adds persistence. Key additions:

1. Thread creation with `thread_created` SSE event
2. Conversation history in the Groq prompt
3. Source labeling that distinguishes transcript vs chat sources
4. Post-stream save of assistant message + sources
5. Enqueue `JOB_VECTORIZE_CHAT_QA` job
6. Concurrent title generation with `thread_title` SSE event

Reference the existing `chat_stream` in `search.rs` (lines 25-215) for the SSE streaming pattern, Groq API call format, and source formatting. The new handler replaces it entirely.

- [ ] **Step 2: Remove old chat_stream from search.rs**

Remove the `chat_stream` function and `ChatRequest` struct from `search.rs`.

- [ ] **Step 3: Update router.rs imports**

Change the chat route import from `search::chat_stream` to `chat::chat_stream`. Keep the route path the same: `/api/v1/chat`.

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: compiles successfully

- [ ] **Step 5: Test manually**

Run: `cargo run`
Test with curl:
```bash
# Create new thread (no thread_id)
curl -X POST http://localhost:8080/api/v1/chat \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"query": "What was discussed in the last meeting?"}'

# Expected: SSE stream with thread_created, answer_chunk, thread_title, done events

# List threads
curl http://localhost:8080/api/v1/chat/threads?limit=3 \
  -H "Authorization: Bearer <token>"

# Expected: {"threads": [{"id": "...", "title": "...", ...}]}
```

- [ ] **Step 6: Commit**

```bash
git add src/routes/chat.rs src/routes/search.rs src/routes/router.rs
git commit -m "feat: add persistent chat_stream with thread history and vectorization"
```

---

## Chunk 4: Frontend

### Task 9: Add TypeScript types and API client methods

**Files:**
- Modify: `frontend/src/lib/types/meetings.ts`
- Modify: `frontend/src/lib/backend_connection/client.ts`

- [ ] **Step 1: Add chat types**

Add to `frontend/src/lib/types/meetings.ts`:

```typescript
export interface ChatThread {
  id: string;
  user_id: string;
  workspace_id: string;
  title: string | null;
  created_at: string;
  updated_at: string;
}

export interface ChatMessageRecord {
  id: string;
  thread_id: string;
  role: "user" | "assistant";
  content: string;
  sources_json: string | null;
  created_at: string;
}

export interface ThreadsResponse {
  threads: ChatThread[];
}

export interface ThreadMessagesResponse {
  messages: ChatMessageRecord[];
}
```

- [ ] **Step 2: Add API client methods**

Add to `frontend/src/lib/backend_connection/client.ts`:

```typescript
listChatThreads(limit?: number) {
  const params = limit ? `?limit=${limit}` : "";
  return this.request<ThreadsResponse>(`/api/v1/chat/threads${params}`);
}

getThreadMessages(threadId: string, limit?: number, before?: string) {
  const params = new URLSearchParams();
  if (limit) params.set("limit", String(limit));
  if (before) params.set("before", before);
  const qs = params.toString();
  return this.request<ThreadMessagesResponse>(`/api/v1/chat/threads/${threadId}/messages${qs ? `?${qs}` : ""}`);
}

updateThreadTitle(threadId: string, title: string) {
  return this.request<{ success: boolean }>(`/api/v1/chat/threads/${threadId}`, {
    method: "PATCH",
    body: JSON.stringify({ title }),
  });
}

deleteThread(threadId: string) {
  return this.request<{ success: boolean }>(`/api/v1/chat/threads/${threadId}`, {
    method: "DELETE",
  });
}
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/types/meetings.ts frontend/src/lib/backend_connection/client.ts
git commit -m "feat: add chat thread types and API client methods"
```

---

### Task 10: Rewrite chat panel with thread views

**Files:**
- Modify: `frontend/src/components/chat.tsx`

- [ ] **Step 1: Rewrite ChatProvider and panel with three views**

Rewrite `frontend/src/components/chat.tsx` to include:

1. **Thread list view** — default state showing "New Chat" button, 3 recent threads (fetched via `listChatThreads(3)`), and "View all" link that loads all threads
2. **Conversation view** — back button, editable title, messages loaded from API, streaming input
3. **New chat mode** — empty state, first send creates thread via `thread_created` SSE event

Key changes to the streaming logic:
- Parse new SSE event types: `thread_created` (captures `thread_id`), `thread_title` (updates title)
- On `thread_created`, update local state with the new thread ID
- On `thread_title`, update the thread's title in local state
- On `done`, the message is already saved server-side — just update the UI
- Thread list refreshes when navigating back from a conversation

The `ChatProvider` context continues to manage open/close state and the `marginRight` push behavior. Add `activeThreadId` and `view` ("threads" | "conversation" | "new") to the context.

- [ ] **Step 2: Test manually in browser**

1. Open the app, click "Chat AI" button
2. Verify thread list shows (empty initially)
3. Click "New Chat", type a message, send
4. Verify streaming works, thread_created event sets thread_id, title appears
5. Click back arrow, verify thread appears in list
6. Click the thread, verify messages load
7. Send a follow-up, verify conversation history context works
8. Edit the thread title inline
9. Delete a thread

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/chat.tsx
git commit -m "feat: rewrite chat panel with thread list, conversation history, and persistence"
```

---

### Task 11: Clean up old meeting chat tab

**Files:**
- Modify: `frontend/src/components/meetings/meetings-view.tsx`

- [ ] **Step 1: Remove the Chat tab from meetings view**

The "Chat" tab in the meetings view is now redundant since the chat panel is global. Remove the `TabsTrigger` for "chat" and the corresponding `TabsContent` with `MeetingChat`. Also remove the import of `MeetingChat` from `meeting-search`.

- [ ] **Step 2: Commit**

```bash
git add frontend/src/components/meetings/meetings-view.tsx
git commit -m "feat: remove redundant chat tab from meetings view"
```
