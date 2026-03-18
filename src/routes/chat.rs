use std::convert::Infallible;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

use super::{helpers::current_user, state::AppState};
use crate::models::ApiError;
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
    pub meeting_ids: Option<Vec<String>>,
}

// ── Thread CRUD handlers ──────────────────────────────────────────────

pub async fn list_threads(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Query(params): Query<ListThreadsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let limit = params.limit.unwrap_or(50);

    // Try Redis cache first
    if let Some(redis) = &state.services.redis {
        if let Ok(Some(cached)) = redis.get_cached_chat_threads(&user.user_id, limit).await {
            return Ok(axum::response::Response::builder()
                .header("content-type", "application/json")
                .body(axum::body::Body::from(cached))
                .unwrap()
                .into_response());
        }
    }

    let threads = state
        .services
        .turso
        .list_chat_threads(&user.user_id, Some(limit))
        .await?;
    let body = json!({ "threads": threads });

    if let Some(redis) = &state.services.redis {
        let json_str = serde_json::to_string(&body).unwrap_or_default();
        let _ = redis.set_cached_chat_threads(&user.user_id, limit, &json_str).await;
    }

    Ok(Json(body).into_response())
}

pub async fn get_thread_messages(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
    Query(params): Query<GetMessagesQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let limit = params.limit.unwrap_or(50);

    // Only cache the initial page (no cursor)
    if params.before.is_none() {
        if let Some(redis) = &state.services.redis {
            if let Ok(Some(cached)) = redis.get_cached_chat_messages(&thread_id).await {
                return Ok(axum::response::Response::builder()
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(cached))
                    .unwrap()
                    .into_response());
            }
        }
    }

    let messages = state
        .services
        .turso
        .get_chat_messages(&thread_id, &user.user_id, limit, params.before.as_deref())
        .await?;
    let body = json!({ "messages": messages });

    if params.before.is_none() {
        if let Some(redis) = &state.services.redis {
            let json_str = serde_json::to_string(&body).unwrap_or_default();
            let _ = redis.set_cached_chat_messages(&thread_id, &json_str).await;
        }
    }

    Ok(Json(body).into_response())
}

pub async fn update_thread(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
    Json(body): Json<UpdateThreadBody>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let updated = state
        .services
        .turso
        .update_chat_thread_title(&thread_id, &user.user_id, &body.title)
        .await?;
    if !updated {
        return Err(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "thread not found",
        ));
    }
    if let Some(redis) = &state.services.redis {
        redis.invalidate_chat_threads(&user.user_id).await;
    }
    Ok(Json(json!({ "success": true })))
}

pub async fn delete_thread(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Path(thread_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let deleted = state
        .services
        .turso
        .soft_delete_chat_thread(&thread_id, &user.user_id)
        .await?;
    if !deleted {
        return Err(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "thread not found",
        ));
    }
    if let Some(redis) = &state.services.redis {
        redis.invalidate_chat_threads(&user.user_id).await;
        redis.invalidate_chat_thread_messages(&thread_id).await;
    }
    Ok(Json(json!({ "success": true })))
}

// ── Persistent chat_stream handler ────────────────────────────────────

pub async fn chat_stream(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Json(payload): Json<ChatRequest>,
) -> Result<axum::response::Response, ApiError> {
    let user = current_user(&state, &jwt).await?;

    let jina = state
        .services
        .jina
        .as_ref()
        .ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "Jina is not configured",
            )
        })?
        .clone();
    let qdrant = state
        .services
        .qdrant
        .as_ref()
        .ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "Qdrant is not configured",
            )
        })?
        .clone();
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

    let query = payload.query.clone();
    let user_id = user.user_id.clone();
    let workspace_id = user.workspace_id.clone();
    let meeting_id = payload.meeting_id.clone();
    let meeting_ids = payload.meeting_ids.clone();
    let config = state.services.config.clone();
    let turso = state.services.turso.clone();
    let redis = state.services.redis.clone();
    let is_new_thread = payload.thread_id.is_none();

    // 1. Create or validate thread
    let thread = if let Some(ref tid) = payload.thread_id {
        let existing = turso.get_chat_thread(tid, &user_id).await.map_err(|e| {
            ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        match existing {
            Some(t) => t,
            None => {
                return Err(ApiError::new(
                    axum::http::StatusCode::NOT_FOUND,
                    "thread not found",
                ));
            }
        }
    } else {
        turso
            .create_chat_thread(&user_id, &workspace_id)
            .await
            .map_err(|e| {
                ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            })?
    };
    let thread_id = thread.id.clone();

    // 2. Save user message
    turso
        .insert_chat_message(&thread_id, "user", &query, None)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Invalidate caches after user message insert
    if let Some(ref redis) = redis {
        redis.invalidate_chat_thread_messages(&thread_id).await;
        redis.invalidate_chat_threads(&user_id).await;
    }

    // 3. Load conversation history
    let history = turso
        .get_recent_thread_messages(&thread_id, 20)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 4. Embed the query
    let query_vectors = jina
        .embed(vec![query.clone()], "retrieval.query")
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let query_vector = query_vectors.into_iter().next().ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "no embedding returned",
        )
    })?;

    // 5. Search both collections in parallel
    // Combine single meeting_id with meeting_ids array
    let effective_meeting_ids: Option<Vec<String>> = match (&meeting_id, &meeting_ids) {
        (Some(id), Some(ids)) => {
            let mut combined = ids.clone();
            if !combined.contains(id) {
                combined.push(id.clone());
            }
            Some(combined)
        }
        (Some(id), None) => Some(vec![id.clone()]),
        (None, Some(ids)) if !ids.is_empty() => Some(ids.clone()),
        _ => None,
    };

    let has_meeting_filter = effective_meeting_ids.is_some();
    let filter_user = if has_meeting_filter {
        None
    } else {
        Some(user_id.as_str())
    };

    let (transcript_results, chat_results) = tokio::join!(
        qdrant.hybrid_search(query_vector.clone(), filter_user, effective_meeting_ids.as_deref(), 150),
        qdrant_chat.hybrid_search(query_vector, filter_user, None, 150),
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

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    // --- FIX: use a oneshot channel so the title is guaranteed to be sent
    // before the `done` event, eliminating the race condition where the SSE
    // channel could be dropped before the title spawn had a chance to write.
    //
    // Flow for new threads:
    //   1. Title spawn runs generate_title(), persists to DB, sends result
    //      back via `title_tx`.
    //   2. Main streaming spawn receives the title via `title_rx` (with a
    //      10 s timeout) just before it sends `done`.
    //   3. It emits `thread_title` then `done` — always in that order,
    //      always on the same channel, with no interleaving.
    // ---

    // Send thread_created event for new threads
    if is_new_thread {
        let _ = tx
            .send(Ok(Event::default().data(
                json!({"type": "thread_created", "thread_id": thread_id}).to_string(),
            )))
            .await;
    }

    // Kick off title generation in a background task only for new threads.
    // The oneshot sender is moved into the spawn; the receiver is forwarded
    // to the main streaming spawn below.
    let title_rx_opt = if is_new_thread {
        let (title_tx, title_rx) = tokio::sync::oneshot::channel::<String>();

        let turso_title = turso.clone();
        let config_title = config.clone();
        let query_title = query.clone();
        let thread_id_title = thread_id.clone();
        let user_id_title = user_id.clone();
        let redis_title = redis.clone();

        tokio::spawn(async move {
            let title = generate_title(
                &turso_title,
                &config_title,
                &query_title,
                &thread_id_title,
                &user_id_title,
                redis_title.as_ref(),
            )
            .await;
            let _ = title_tx.send(title);
        });

        Some(title_rx)
    } else {
        None
    };

    if search_results.is_empty() {
        let thread_id_bg = thread_id.clone();
        let turso_bg = turso.clone();
        let redis_bg = redis.clone();
        let user_id_bg = user_id.clone();
        tokio::spawn(async move {
            let no_results_msg =
                "No relevant transcript content found for your question.".to_string();
            let _ = tx
                .send(Ok(Event::default().data(
                    json!({"type": "answer_chunk", "content": no_results_msg}).to_string(),
                )))
                .await;

            // Wait for the title before sending `done` so the client always
            // receives `thread_title` first on new threads.
            if let Some(rx) = title_rx_opt {
                if let Ok(Ok(title)) = tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
                    let _ = tx
                        .send(Ok(Event::default().data(
                            json!({"type": "thread_title", "title": title}).to_string(),
                        )))
                        .await;
                }
            }

            let _ = tx
                .send(Ok(
                    Event::default().data(json!({"type": "done", "sources": []}).to_string())
                ))
                .await;

            // Save the assistant message
            if let Err(e) = turso_bg
                .insert_chat_message(
                    &thread_id_bg,
                    "assistant",
                    "No relevant transcript content found for your question.",
                    Some("[]"),
                )
                .await
            {
                error!(error = %e, "failed to save assistant message");
            }

            if let Some(ref redis) = redis_bg {
                redis.invalidate_chat_thread_messages(&thread_id_bg).await;
                redis.invalidate_chat_threads(&user_id_bg).await;
            }
        });

        return Ok(Sse::new(ReceiverStream::new(rx))
            .keep_alive(KeepAlive::default())
            .into_response());
    }

    // 6. Rerank via Jina
    let documents: Vec<String> = search_results.iter().map(|r| r.text.clone()).collect();
    let reranked = jina
        .rerank(&query, documents, 5)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let top_results: Vec<_> = reranked
        .results
        .iter()
        .filter_map(|r| {
            let original = search_results.get(r.index)?;
            Some((original.clone(), r.relevance_score))
        })
        .collect();

    let sources_json: Vec<Value> = top_results
        .iter()
        .map(|(result, score)| {
            json!({
                "source_type": result.source_type,
                "meeting_id": result.meeting_id.clone().unwrap_or_default(),
                "meeting_title": result.meeting_title.clone().unwrap_or_default(),
                "text": result.text,
                "start_ms": result.start_ms.unwrap_or(0),
                "end_ms": result.end_ms.unwrap_or(0),
                "speaker_label": result.speaker_label,
                "thread_id": result.thread_id.clone().unwrap_or_default(),
                "created_at": result.created_at.clone().unwrap_or_default(),
                "relevance_score": score,
            })
        })
        .collect();

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

    // 7. Build messages array with conversation history
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

    let mut messages_array: Vec<Value> = Vec::new();
    messages_array.push(json!({
        "role": "system",
        "content": system_prompt
    }));

    // Add conversation history (exclude the user message we just inserted, which is the last one)
    // History is in chronological order; skip the last message since it's the one we just saved
    let history_to_include =
        if !history.is_empty() && history.last().map(|m| m.role.as_str()) == Some("user") {
            &history[..history.len() - 1]
        } else {
            &history
        };
    for msg in history_to_include {
        messages_array.push(json!({
            "role": msg.role,
            "content": msg.content
        }));
    }

    // Add the final user message with search context
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

    // 8. Stream from Groq in background task
    let groq_api_key = config.groq.api_key.clone().unwrap_or_default();
    let groq_base_url = config.groq.api_base_url.clone();
    let groq_model = config.groq.notes_model.clone();
    let sources_json_for_save = sources_json.clone();
    let thread_id_bg = thread_id.clone();
    let turso_bg = turso.clone();
    let query_bg = query.clone();
    let user_id_bg = user_id.clone();
    let redis_bg = redis.clone();

    tokio::spawn(async move {
        use futures_util::StreamExt;

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let response = http
            .post(format!("{}/openai/v1/chat/completions", groq_base_url))
            .bearer_auth(&groq_api_key)
            .json(&json!({
                "model": groq_model,
                "stream": true,
                "messages": messages_array
            }))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let _ = tx
                    .send(Ok(Event::default().data(
                        json!({"type": "error", "content": e.to_string()}).to_string(),
                    )))
                    .await;
                return;
            }
        };

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut full_answer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break,
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].trim().to_owned();
                buffer = buffer[newline_pos + 1..].to_owned();

                if !line.starts_with("data: ") {
                    continue;
                }
                let data = &line[6..];
                if data == "[DONE]" {
                    // Save assistant message first
                    let sources_str = serde_json::to_string(&sources_json).unwrap_or_default();
                    if let Err(e) = turso_bg
                        .insert_chat_message(
                            &thread_id_bg,
                            "assistant",
                            &full_answer,
                            Some(&sources_str),
                        )
                        .await
                    {
                        error!(error = %e, "failed to save assistant message");
                    }

                    // Enqueue vectorization job
                    if let Err(e) = turso_bg
                        .enqueue_job(
                            JOB_VECTORIZE_CHAT_QA,
                            None,
                            &json!({
                                "thread_id": thread_id_bg,
                                "question": query_bg,
                                "answer": full_answer,
                                "user_id": user_id_bg,
                            }),
                        )
                        .await
                    {
                        error!(error = %e, "failed to enqueue vectorize_chat_qa job");
                    }

                    // Invalidate caches after assistant message saved
                    if let Some(ref redis) = redis_bg {
                        redis.invalidate_chat_thread_messages(&thread_id_bg).await;
                        redis.invalidate_chat_threads(&user_id_bg).await;
                    }

                    // Wait for the title before sending `done` so the client
                    // always receives `thread_title` first on new threads.
                    // We apply a 10 s timeout so a slow/failed LLM call for
                    // the title never hangs the stream indefinitely.
                    if let Some(rx) = title_rx_opt {
                        if let Ok(Ok(title)) = tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            rx,
                        )
                        .await
                        {
                            let _ = tx
                                .send(Ok(Event::default().data(
                                    json!({"type": "thread_title", "title": title})
                                        .to_string(),
                                )))
                                .await;
                        }
                    }

                    // Send done event
                    let _ = tx
                        .send(Ok(Event::default().data(
                            json!({"type": "done", "sources": sources_json_for_save}).to_string(),
                        )))
                        .await;

                    return;
                }
                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                    if let Some(content) = parsed
                        .pointer("/choices/0/delta/content")
                        .and_then(Value::as_str)
                    {
                        if !content.is_empty() {
                            full_answer.push_str(content);
                            let _ = tx
                                .send(Ok(Event::default().data(
                                    json!({"type": "answer_chunk", "content": content}).to_string(),
                                )))
                                .await;
                        }
                    }
                }
            }
        }

        // If we get here without [DONE], still save what we have and emit
        // the title + done so the client is never left hanging.
        if !full_answer.is_empty() {
            let sources_str = serde_json::to_string(&sources_json).unwrap_or_default();
            if let Err(e) = turso_bg
                .insert_chat_message(&thread_id_bg, "assistant", &full_answer, Some(&sources_str))
                .await
            {
                error!(error = %e, "failed to save assistant message (stream ended)");
            }

            if let Err(e) = turso_bg
                .enqueue_job(
                    JOB_VECTORIZE_CHAT_QA,
                    None,
                    &json!({
                        "thread_id": thread_id_bg,
                        "question": query_bg,
                        "answer": full_answer,
                        "user_id": user_id_bg,
                    }),
                )
                .await
            {
                error!(error = %e, "failed to enqueue vectorize_chat_qa job (stream ended)");
            }

            if let Some(ref redis) = redis_bg {
                redis.invalidate_chat_thread_messages(&thread_id_bg).await;
                redis.invalidate_chat_threads(&user_id_bg).await;
            }
        }

        // Best-effort title flush on abnormal stream end
        if let Some(rx) = title_rx_opt {
            if let Ok(Ok(title)) = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
                let _ = tx
                    .send(Ok(Event::default().data(
                        json!({"type": "thread_title", "title": title}).to_string(),
                    )))
                    .await;
            }
        }

        let _ = tx
            .send(Ok(Event::default().data(
                json!({"type": "done", "sources": sources_json_for_save}).to_string(),
            )))
            .await;
    });

    Ok(Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
        .into_response())
}

// ── Helpers ───────────────────────────────────────────────────────────

fn format_ms(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}

/// Generates a short thread title via Groq, persists it to the DB, and
/// invalidates the threads cache. Returns the final title string so the
/// caller can forward it over SSE.
///
/// This is intentionally a plain `async fn` (not a spawn) so that the
/// oneshot sender in the caller drives the lifecycle — no `tx` clone is
/// needed here.
async fn generate_title(
    turso: &crate::service::turso::client::TursoClient,
    config: &crate::config::AppConfig,
    query: &str,
    thread_id: &str,
    user_id: &str,
    redis: Option<&crate::service::redis::RedisClient>,
) -> String {
    let groq_api_key = config.groq.api_key.clone().unwrap_or_default();
    let groq_base_url = config.groq.api_base_url.clone();

    let title_model = "llama-3.1-8b-instant".to_string();

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let result = http
        .post(format!("{}/openai/v1/chat/completions", groq_base_url))
        .bearer_auth(&groq_api_key)
        .json(&json!({
            "model": title_model,
            "messages": [
                {
                    "role": "system",
                    "content": "Generate a short, descriptive title (max 6 words) for a chat conversation that starts with the following question. Return ONLY the title text, nothing else."
                },
                {
                    "role": "user",
                    "content": query
                }
            ],
            "max_tokens": 30,
            "temperature": 0.5
        }))
        .send()
        .await;

    let title = match result {
        Ok(resp) => {
            match resp.text().await {
                Ok(raw) => {
                    serde_json::from_str::<Value>(&raw)
                        .ok()
                        .and_then(|body| {
                            body.pointer("/choices/0/message/content")
                                .and_then(Value::as_str)
                                .map(|s| s.trim().trim_matches('"').to_owned())
                                .filter(|s| !s.is_empty())
                        })
                }
                Err(e) => {
                    error!(error = %e, "generate_title: failed to read Groq response body");
                    None
                }
            }
        }
        Err(e) => {
            error!(error = %e, "generate_title: Groq HTTP request failed");
            None
        }
    };

    let title = title.unwrap_or_else(|| {
        let truncated: String = query.chars().take(50).collect();
        if query.chars().count() > 50 {
            format!("{}...", truncated)
        } else {
            truncated
        }
    });

    if let Err(e) = turso
        .update_chat_thread_title(thread_id, user_id, &title)
        .await
    {
        error!(error = %e, "generate_title: failed to update thread title in DB");
    }

    if let Some(redis) = redis {
        redis.invalidate_chat_threads(user_id).await;
    }

    title
}
