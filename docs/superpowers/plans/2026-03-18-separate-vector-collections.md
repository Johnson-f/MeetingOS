# Separate Vector Collections for Transcripts and Chat Conversations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the single `meeting_transcripts` Qdrant collection into two separate collections — one for transcript chunks and one for chat Q&A pairs — then update the search flow to query both in parallel and present labeled context to the LLM.

**Architecture:** The current system stores both transcript chunks and chat Q&A pairs in a single Qdrant collection (`meeting_transcripts`), differentiated by a `source_type` payload field. We will create a second collection (`chat_conversations`) for chat data, update the `QdrantClient` to manage both collections, modify the search flow in `chat_stream` to query both collections in parallel, and update the LLM system prompt to clearly distinguish between the two context sources. The existing `meeting_transcripts` collection schema stays unchanged — we just stop writing chat data to it.

**Tech Stack:** Rust, Qdrant (client 1.17), Jina embeddings (v3, 1024-dim), Axum, Tokio

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/config.rs` | Modify | Add `chat_collection_name` field to `QdrantConfig` |
| `src/service/qdrant_search/client.rs` | Modify | Add second collection init, split upsert/search methods, add parallel search |
| `src/service/qdrant_search/mod.rs` | Modify | Re-export new types if needed |
| `src/service/vector/search.rs` | Modify | Update `vectorize_chat_qa` to target the chat collection |
| `src/routes/chat.rs` | Modify | Search both collections in parallel, build separate context blocks, update system prompt |
| `src/service/mod.rs` | Modify | Add `qdrant_chat` field to `ServiceRegistry` |

---

### Task 1: Add chat collection config

**Files:**
- Modify: `src/config.rs:75-80` (QdrantConfig struct)
- Modify: `src/config.rs:181-186` (from_env)

- [ ] **Step 1: Add `chat_collection_name` to `QdrantConfig`**

In `src/config.rs`, add a new field to `QdrantConfig`:

```rust
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub collection_name: String,
    pub chat_collection_name: String,
}
```

- [ ] **Step 2: Load from env with default**

In the `from_env()` method, update the `qdrant` block:

```rust
qdrant: QdrantConfig {
    url: env::var("QDRANT_URL").ok(),
    api_key: env::var("QDRANT_API_KEY").ok(),
    collection_name: env::var("QDRANT_COLLECTION")
        .unwrap_or_else(|_| "meeting_transcripts".to_owned()),
    chat_collection_name: env::var("QDRANT_CHAT_COLLECTION")
        .unwrap_or_else(|_| "chat_conversations".to_owned()),
},
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Should compile (no consumers of the new field yet).

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat: add chat_collection_name to QdrantConfig"
```

---

### Task 2: Add a second QdrantClient instance for the chat collection

**Files:**
- Modify: `src/service/mod.rs:25-72` (ServiceRegistry)

- [ ] **Step 1: Add `qdrant_chat` to `ServiceRegistry`**

In `src/service/mod.rs`, add a new field:

```rust
#[derive(Clone)]
pub struct ServiceRegistry {
    pub turso: TursoClient,
    pub recall_ai: Option<RecallAiClient>,
    pub storage: Option<StorageClient>,
    pub redis: Option<RedisClient>,
    pub jina: Option<JinaClient>,
    pub qdrant: Option<QdrantClient>,
    pub qdrant_chat: Option<QdrantClient>,
    pub google_calendar: Option<GoogleCalendarClient>,
    pub config: AppConfig,
    pub sse_tx: broadcast::Sender<SseEvent>,
}
```

- [ ] **Step 2: Initialize `qdrant_chat` in `ServiceRegistry::new`**

Create a second `QdrantConfig` with the chat collection name and connect a second client:

```rust
let qdrant = QdrantClient::connect(&config.qdrant).await;

// Chat collection uses same Qdrant server, different collection
let chat_qdrant_config = crate::config::QdrantConfig {
    url: config.qdrant.url.clone(),
    api_key: config.qdrant.api_key.clone(),
    collection_name: config.qdrant.chat_collection_name.clone(),
};
let qdrant_chat = QdrantClient::connect(&chat_qdrant_config).await;
```

Update the logging:

```rust
info!(
    recall_ai = recall_ai.is_some(),
    storage = storage.is_some(),
    redis = redis.is_some(),
    jina = jina.is_some(),
    qdrant = qdrant.is_some(),
    qdrant_chat = qdrant_chat.is_some(),
    google_calendar = google_calendar.is_some(),
    groq = config.groq.api_key.is_some(),
    "services initialized"
);
```

And include it in the struct construction:

```rust
Self {
    turso,
    recall_ai,
    storage,
    redis,
    jina,
    qdrant,
    qdrant_chat,
    google_calendar,
    config,
    sse_tx,
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Should compile. The `qdrant_chat` client will auto-create the `chat_conversations` collection on first connect via `ensure_collection()`.

- [ ] **Step 4: Commit**

```bash
git add src/service/mod.rs
git commit -m "feat: add qdrant_chat client to ServiceRegistry"
```

---

### Task 3: Add `thread_id` index to chat collection

The chat collection needs a `thread_id` keyword index for scoped searches. The existing `ensure_indexes` method creates `user_id`, `meeting_id`, and `source_type` indexes — these are fine for the transcript collection but the chat collection needs `thread_id` instead of `meeting_id`.

**Files:**
- Modify: `src/service/qdrant_search/client.rs:63-148`

- [ ] **Step 1: Make `ensure_indexes` configurable**

Replace the hardcoded field list with a parameter passed during `connect`. The simplest approach: add a `chat_mode` flag or make `ensure_indexes` accept a field list. Since `QdrantClient` is reused for both collections, the cleanest approach is to check the collection name or pass a field list.

Update `ensure_indexes` to accept a list of fields:

```rust
async fn ensure_indexes(&self, fields: &[&str]) -> Result<()> {
    for field in fields {
        let result = self
            .client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                &self.collection_name,
                *field,
                FieldType::Keyword,
            ))
            .await;
        if let Err(e) = &result {
            let msg = e.to_string();
            if !msg.contains("already exists") {
                result.with_context(|| format!("failed to create index on {}", field))?;
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Update `ensure_collection` to pass appropriate fields**

Change `connect` to accept an optional list of index fields, defaulting to the transcript set:

```rust
pub async fn connect(config: &QdrantConfig) -> Option<Self> {
    Self::connect_with_indexes(config, &["user_id", "meeting_id", "source_type"]).await
}

pub async fn connect_for_chat(config: &QdrantConfig) -> Option<Self> {
    Self::connect_with_indexes(config, &["user_id", "thread_id", "source_type"]).await
}

async fn connect_with_indexes(config: &QdrantConfig, index_fields: &[&str]) -> Option<Self> {
    let url = config.url.as_deref()?;

    let mut builder = Qdrant::from_url(url).skip_compatibility_check();
    if let Some(api_key) = &config.api_key {
        builder = builder.api_key(api_key.as_str());
    }

    match builder.build() {
        Ok(client) => {
            info!("Qdrant client connected to {}", url);
            let qdrant = Self {
                client,
                collection_name: config.collection_name.clone(),
            };
            if let Err(e) = qdrant.ensure_collection(index_fields).await {
                warn!(error = %e, "failed to ensure Qdrant collection");
                return None;
            }
            Some(qdrant)
        }
        Err(e) => {
            warn!(error = %e, "failed to create Qdrant client");
            None
        }
    }
}
```

Update `ensure_collection` signature:

```rust
async fn ensure_collection(&self, index_fields: &[&str]) -> Result<()> {
    // ... existing collection creation logic ...
    self.ensure_indexes(index_fields).await?;
    // ...
}
```

- [ ] **Step 3: Update `ServiceRegistry::new` to use `connect_for_chat`**

In `src/service/mod.rs`:

```rust
let qdrant_chat = QdrantClient::connect_for_chat(&chat_qdrant_config).await;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/service/qdrant_search/client.rs src/service/mod.rs
git commit -m "feat: configurable indexes per Qdrant collection"
```

---

### Task 4: Route chat Q&A vectorization to the chat collection

**Files:**
- Modify: `src/service/vector/search.rs:78-111`

- [ ] **Step 1: Update `vectorize_chat_qa` to use `qdrant_chat`**

Change the function to pull from `services.qdrant_chat` instead of `services.qdrant`:

```rust
pub async fn vectorize_chat_qa(
    services: &ServiceRegistry,
    thread_id: &str,
    user_id: &str,
    question: &str,
    answer: &str,
) -> Result<()> {
    if answer.len() < 50 || answer.starts_with("Error:") {
        tracing::info!(thread_id = %thread_id, "skipping chat QA vectorization: answer too short or error");
        return Ok(());
    }

    let jina = services.jina.as_ref().context("jina not configured")?;
    let qdrant = services.qdrant_chat.as_ref().context("qdrant chat collection not configured")?;

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
    tracing::info!(thread_id = %thread_id, "chat QA vectorized to chat collection");
    Ok(())
}
```

The only change is `services.qdrant` → `services.qdrant_chat` and the context message.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/service/vector/search.rs
git commit -m "feat: route chat Q&A vectorization to chat_conversations collection"
```

---

### Task 5: Parallel search across both collections

This is the core change. The `chat_stream` handler currently does a single `hybrid_search` against one collection. We need to search both collections in parallel and merge the results.

**Files:**
- Modify: `src/routes/chat.rs:120-216`

- [ ] **Step 1: Get `qdrant_chat` client in the handler**

After the existing `qdrant` extraction (around line 139-149), add:

```rust
let qdrant_chat = state
    .services
    .qdrant_chat
    .as_ref()
    .ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Qdrant chat collection is not configured",
        )
    })?
    .clone();
```

- [ ] **Step 2: Replace single search with parallel search**

Replace the current search block (lines 207-216) with parallel searches using `tokio::join!`:

```rust
// 5. Search both collections in parallel
let filter_user = if meeting_id.is_some() {
    None
} else {
    Some(user_id.as_str())
};

let (transcript_results, chat_results) = tokio::join!(
    qdrant.hybrid_search(query_vector.clone(), filter_user, 15),
    qdrant_chat.hybrid_search(query_vector, filter_user, 15),
);

let transcript_results = transcript_results.map_err(|e| {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
})?;
let chat_results = chat_results.map_err(|e| {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
})?;

// Merge results, keeping source_type for downstream differentiation
let mut search_results = transcript_results;
search_results.extend(chat_results);

info!(
    transcript_count = search_results.iter().filter(|r| r.source_type == "transcript").count(),
    chat_count = search_results.iter().filter(|r| r.source_type == "chat").count(),
    "Qdrant parallel search for chat"
);
```

Note: `query_vector` needs to be cloned since it's moved into the first future. Add `.clone()` before passing to the first search.

- [ ] **Step 3: Check that the rest of the handler still works**

The downstream code (reranking at line 288, context building at line 322, etc.) already handles `source_type == "chat"` vs `"transcript"` in the `context_chunks` builder. No changes needed there — the `search_results` vec just now contains results from two collections instead of one.

- [ ] **Step 4: Handle `search_results.is_empty()` correctly**

The existing empty-results check at line 231 still works — if both collections return nothing, the combined vec is empty.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Compiles cleanly.

- [ ] **Step 6: Commit**

```bash
git add src/routes/chat.rs
git commit -m "feat: search transcript and chat collections in parallel"
```

---

### Task 6: Update the LLM system prompt to differentiate sources

**Files:**
- Modify: `src/routes/chat.rs:322-378`

- [ ] **Step 1: Split context into labeled blocks**

Replace the `context_chunks` builder and system prompt. Currently the context is built as a single block. Split it into two labeled sections:

```rust
let transcript_context: String = top_results
    .iter()
    .enumerate()
    .filter(|(_, (result, _))| result.source_type != "chat")
    .map(|(i, (result, _))| {
        let speaker = result.speaker_label.as_deref().unwrap_or("Unknown");
        let timestamp = format_ms(result.start_ms.unwrap_or(0));
        let meeting_id_str = result.meeting_id.clone().unwrap_or_default();
        let meeting_title = result.meeting_title.clone().unwrap_or_default();
        let title = if meeting_title.is_empty() {
            &meeting_id_str
        } else {
            &meeting_title
        };
        format!(
            "[Source {}] (Meeting: {}, Speaker: {}, Time: {})\n{}",
            i + 1,
            title,
            speaker,
            timestamp,
            result.text
        )
    })
    .collect::<Vec<_>>()
    .join("\n\n");

let chat_context: String = top_results
    .iter()
    .enumerate()
    .filter(|(_, (result, _))| result.source_type == "chat")
    .map(|(i, (result, _))| {
        let thread = result.thread_id.as_deref().unwrap_or("unknown");
        let date = result.created_at.as_deref().unwrap_or("unknown");
        format!(
            "[Source {}] (Previous conversation, thread: {}, date: {})\n{}",
            i + 1,
            thread,
            date,
            result.text
        )
    })
    .collect::<Vec<_>>()
    .join("\n\n");
```

- [ ] **Step 2: Update the system prompt**

```rust
let system_prompt = "You are a meeting Q&A assistant. You have access to two types of sources:\n\n\
1. **Meeting Transcripts** — Excerpts from the user's recorded meetings.\n\
2. **Previous Conversations** — Past Q&A exchanges between you and this user.\n\n\
Rules:\n\
- Only use information explicitly present in the sources. Never make up information.\n\
- Reference which source number (e.g. [Source 1]) supports your answer.\n\
- Prioritize meeting transcript sources for factual meeting content.\n\
- Use previous conversation sources when the user asks about past discussions or to build on prior answers.\n\
- If the answer is not in any provided source, say: \"I couldn't find information about that in your meetings or our past conversations.\"\n\
- Be concise and direct. 2-4 sentences unless the question requires more detail.\n\
- If a speaker is identified, mention them by name.";
```

- [ ] **Step 3: Update the user message with separated context**

```rust
let mut context_parts = Vec::new();
if !transcript_context.is_empty() {
    context_parts.push(format!("## Meeting Transcript Excerpts\n\n{}", transcript_context));
}
if !chat_context.is_empty() {
    context_parts.push(format!("## Previous Conversations\n\n{}", chat_context));
}
let full_context = context_parts.join("\n\n---\n\n");

messages_array.push(json!({
    "role": "user",
    "content": format!("{}\n\n## Question\n\n{}", full_context, query)
}));
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: Compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/routes/chat.rs
git commit -m "feat: separate transcript and chat context in LLM prompt"
```

---

### Task 7: Migrate existing chat data out of `meeting_transcripts` (optional cleanup)

This is a one-time cleanup. Existing chat Q&A points in the `meeting_transcripts` collection should be migrated to `chat_conversations`. This can be done via a small migration script or a one-off job.

**Files:**
- No new files — use a one-off Qdrant API call

- [ ] **Step 1: Write a migration function**

Add a temporary helper in `src/service/qdrant_search/client.rs`:

```rust
/// One-time migration: scroll all chat points from this collection,
/// return them so they can be re-inserted into the chat collection.
pub async fn extract_chat_points(&self) -> Result<Vec<ChatQAPoint>> {
    use qdrant_client::qdrant::{ScrollPointsBuilder, Condition, Filter};

    let filter = Filter::must([Condition::matches("source_type", "chat".to_owned())]);

    let response = self
        .client
        .scroll(
            ScrollPointsBuilder::new(&self.collection_name)
                .filter(filter)
                .limit(1000)
                .with_payload(true)
                .with_vectors(true),
        )
        .await
        .context("failed to scroll chat points")?;

    let points = response
        .result
        .into_iter()
        .filter_map(|point| {
            let payload = &point.payload;
            let text = payload.get("text")?.as_str()?.to_owned();
            let user_id = payload.get("user_id")?.as_str()?.to_owned();
            let thread_id = payload.get("thread_id")?.as_str()?.to_owned();
            let created_at = payload
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            // Extract dense vector
            let vectors = point.vectors?;
            let dense_vector = match vectors.vectors_options? {
                qdrant_client::qdrant::vectors::VectorsOptions::Vectors(named) => {
                    named.vectors.get("dense")?.data.clone()
                }
                _ => return None,
            };

            Some(ChatQAPoint {
                id: uuid::Uuid::new_v4().to_string(),
                user_id,
                thread_id,
                text,
                created_at,
                dense_vector,
            })
        })
        .collect();

    Ok(points)
}

/// Delete all chat points from this (transcript) collection.
pub async fn delete_chat_points(&self) -> Result<()> {
    self.client
        .delete_points(
            DeletePointsBuilder::new(&self.collection_name)
                .points(Filter::must([Condition::matches(
                    "source_type",
                    "chat".to_owned(),
                )]))
                .wait(true),
        )
        .await
        .context("failed to delete chat points from transcript collection")?;
    info!("deleted chat points from transcript collection");
    Ok(())
}
```

- [ ] **Step 2: Add a migration job constant**

In `src/service/jobs/constants.rs`:

```rust
pub const JOB_MIGRATE_CHAT_VECTORS: &str = "migrate_chat_vectors";
```

- [ ] **Step 3: Add the migration handler**

In `src/service/jobs/handlers.rs`, add:

```rust
pub(super) async fn migrate_chat_vectors_job(
    services: &ServiceRegistry,
    _payload: &Value,
) -> Result<()> {
    let qdrant_transcripts = services.qdrant.as_ref().context("qdrant not configured")?;
    let qdrant_chat = services.qdrant_chat.as_ref().context("qdrant_chat not configured")?;

    info!("migrating chat vectors from transcript collection to chat collection");

    let chat_points = qdrant_transcripts.extract_chat_points().await?;
    if chat_points.is_empty() {
        info!("no chat points to migrate");
        return Ok(());
    }

    info!(count = chat_points.len(), "extracted chat points, upserting to chat collection");
    qdrant_chat.upsert_chat_qa_points(chat_points).await?;

    qdrant_transcripts.delete_chat_points().await?;
    info!("chat vector migration complete");
    Ok(())
}
```

- [ ] **Step 4: Wire the migration handler in the job runner**

Find where job types are matched in `src/service/jobs/runner.rs` and add:

```rust
JOB_MIGRATE_CHAT_VECTORS => migrate_chat_vectors_job(services, payload).await,
```

Make sure to import the constant.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | head -20`

- [ ] **Step 6: Commit**

```bash
git add src/service/qdrant_search/client.rs src/service/jobs/constants.rs src/service/jobs/handlers.rs src/service/jobs/runner.rs
git commit -m "feat: add one-time migration job for chat vectors"
```

- [ ] **Step 7: Trigger the migration**

After deploying, enqueue a single migration job via the Turso database or an admin endpoint. Once confirmed, the `extract_chat_points` and `delete_chat_points` methods can be removed in a future cleanup.

---

### Task 8: Smoke test the full flow

- [ ] **Step 1: Build the project**

Run: `cargo build 2>&1 | tail -5`
Expected: Successful build with no errors.

- [ ] **Step 2: Verify both collections are created on startup**

Run the binary locally (or check logs). You should see two log lines:
```
Qdrant client connected to http://localhost:6334
...created Qdrant collection with indexes  (collection = "meeting_transcripts")
Qdrant client connected to http://localhost:6334
...created Qdrant collection with indexes  (collection = "chat_conversations")
```

- [ ] **Step 3: Test chat Q&A vectorization path**

Send a chat message via the API. Check Qdrant to verify the Q&A point lands in the `chat_conversations` collection (not `meeting_transcripts`):

```bash
curl -s http://localhost:6334/collections/chat_conversations/points/scroll \
  -H 'Content-Type: application/json' \
  -d '{"limit": 5, "with_payload": true}' | jq '.result.points | length'
```

- [ ] **Step 4: Test parallel search**

Send a chat query and verify logs show both collections being searched:
```
executing Qdrant hybrid search (collection = "meeting_transcripts")
executing Qdrant hybrid search (collection = "chat_conversations")
Qdrant parallel search for chat (transcript_count = X, chat_count = Y)
```

- [ ] **Step 5: Commit any fixes**

```bash
git add -A && git commit -m "fix: address issues from smoke testing"
```
