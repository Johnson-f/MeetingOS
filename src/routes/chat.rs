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
use tracing::{error, info};

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
}

// ── Thread CRUD handlers ──────────────────────────────────────────────

pub async fn list_threads(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Query(params): Query<ListThreadsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let user = current_user(&state, &jwt).await?;
    let threads = state
        .services
        .turso
        .list_chat_threads(&user.user_id, params.limit)
        .await?;
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
    let messages = state
        .services
        .turso
        .get_chat_messages(&thread_id, &user.user_id, limit, params.before.as_deref())
        .await?;
    Ok(Json(json!({ "messages": messages })))
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
    Ok(Json(json!({ "success": true })))
}

// ── Persistent chat_stream handler ────────────────────────────────────

pub async fn chat_stream(
    State(state): State<AppState>,
    Extension(jwt): Extension<ClerkJwt>,
    Json(payload): Json<ChatRequest>,
) -> Result<axum::response::Response, ApiError> {
    info!(sub = %jwt.sub, query = %payload.query, "POST /api/v1/chat");
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
    let config = state.services.config.clone();
    let turso = state.services.turso.clone();
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

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    // Send thread_created event for new threads
    if is_new_thread {
        let _ = tx
            .send(Ok(Event::default().data(
                json!({"type": "thread_created", "thread_id": thread_id}).to_string(),
            )))
            .await;
    }

    if search_results.is_empty() {
        let thread_id_bg = thread_id.clone();
        let turso_bg = turso.clone();
        let tx_bg = tx.clone();
        tokio::spawn(async move {
            let no_results_msg =
                "No relevant transcript content found for your question.".to_string();
            let _ = tx
                .send(Ok(Event::default().data(
                    json!({"type": "answer_chunk", "content": no_results_msg}).to_string(),
                )))
                .await;
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
        });

        // Title generation for new thread even with no results
        if is_new_thread {
            let turso_title = turso.clone();
            let config_title = config.clone();
            let query_title = query.clone();
            let thread_id_title = thread_id.clone();
            let user_id_title = user_id.clone();
            tokio::spawn(async move {
                generate_and_send_title(
                    &turso_title,
                    &config_title,
                    &query_title,
                    &thread_id_title,
                    &user_id_title,
                    &tx_bg,
                )
                .await;
            });
        }

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

    let context_chunks: String = top_results
        .iter()
        .enumerate()
        .map(|(i, (result, _))| {
            if result.source_type == "chat" {
                format!("[Source {}] (Past conversation)\n{}", i + 1, result.text)
            } else {
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
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 7. Build messages array with conversation history
    let system_prompt = "You are a meeting Q&A assistant. Answer questions based on meeting transcript excerpts and relevant past conversations provided below.\n\nRules:\n- Only use information explicitly present in the sources. Never make up information.\n- Reference which source number (e.g. [Source 1]) supports your answer.\n- If the answer is not in the provided sources, say: \"I couldn't find information about that in the meeting transcripts.\"\n- Be concise and direct. 2-4 sentences unless the question requires more detail.\n- If a speaker is identified, mention them by name.\n- Sources marked as \"Past conversation\" are from your previous chats with this user. You may reference them but prioritize meeting transcript sources.";

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
    messages_array.push(json!({
        "role": "user",
        "content": format!("## Meeting Transcript Excerpts\n\n{}\n\n## Question\n\n{}", context_chunks, query)
    }));

    // 8. Stream from Groq in background task
    let groq_api_key = config.groq.api_key.clone().unwrap_or_default();
    let groq_base_url = config.groq.api_base_url.clone();
    let groq_model = config.groq.notes_model.clone();
    let sources_json_for_save = sources_json.clone();
    let thread_id_bg = thread_id.clone();
    let turso_bg = turso.clone();
    let config_bg = config.clone();
    let query_bg = query.clone();
    let user_id_bg = user_id.clone();

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
                    // Save assistant message
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

                    // Send done event
                    let _ = tx
                        .send(Ok(Event::default().data(
                            json!({"type": "done", "sources": sources_json_for_save}).to_string(),
                        )))
                        .await;

                    // Title generation for new threads
                    if is_new_thread {
                        let turso_title = turso_bg.clone();
                        let config_title = config_bg.clone();
                        let query_title = query_bg.clone();
                        let thread_id_title = thread_id_bg.clone();
                        let user_id_title = user_id_bg.clone();
                        let tx_title = tx.clone();
                        tokio::spawn(async move {
                            generate_and_send_title(
                                &turso_title,
                                &config_title,
                                &query_title,
                                &thread_id_title,
                                &user_id_title,
                                &tx_title,
                            )
                            .await;
                        });
                    }

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

        // If we get here without [DONE], still save what we have
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
        }

        let _ = tx
            .send(Ok(Event::default().data(
                json!({"type": "done", "sources": sources_json_for_save}).to_string(),
            )))
            .await;

        if is_new_thread {
            let turso_title = turso_bg.clone();
            let config_title = config_bg.clone();
            let query_title = query_bg.clone();
            let thread_id_title = thread_id_bg.clone();
            let user_id_title = user_id_bg.clone();
            let tx_title = tx.clone();
            tokio::spawn(async move {
                generate_and_send_title(
                    &turso_title,
                    &config_title,
                    &query_title,
                    &thread_id_title,
                    &user_id_title,
                    &tx_title,
                )
                .await;
            });
        }
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

async fn generate_and_send_title(
    turso: &crate::service::turso::client::TursoClient,
    config: &crate::config::AppConfig,
    query: &str,
    thread_id: &str,
    user_id: &str,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) {
    let groq_api_key = config.groq.api_key.clone().unwrap_or_default();
    let groq_base_url = config.groq.api_base_url.clone();
    let groq_model = config.groq.notes_model.clone();

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let result = http
        .post(format!("{}/openai/v1/chat/completions", groq_base_url))
        .bearer_auth(&groq_api_key)
        .json(&json!({
            "model": groq_model,
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
        Ok(resp) => match resp.json::<Value>().await {
            Ok(body) => body
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(|s| s.trim().trim_matches('"').to_owned()),
            Err(_) => None,
        },
        Err(_) => None,
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
        error!(error = %e, "failed to update thread title");
    }

    let _ = tx
        .send(Ok(Event::default().data(
            json!({"type": "thread_title", "title": title}).to_string(),
        )))
        .await;
}
