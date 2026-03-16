use std::convert::Infallible;

use axum::{
    Json,
    extract::{Extension, State},
    response::{IntoResponse, sse::{Event, KeepAlive, Sse}},
};
use clerk_rs::validators::authorizer::ClerkJwt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

use crate::models::ApiError;

use super::{helpers::current_user, state::AppState};

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub query: String,
    pub meeting_id: Option<String>,
}

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
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::SERVICE_UNAVAILABLE, "Jina is not configured"))?
        .clone();
    let qdrant = state
        .services
        .qdrant
        .as_ref()
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::SERVICE_UNAVAILABLE, "Qdrant is not configured"))?
        .clone();

    let query = payload.query.clone();
    let user_id = user.user_id.clone();
    let meeting_id = payload.meeting_id.clone();
    let config = state.services.config.clone();

    // 1. Embed the query
    let query_vectors = jina
        .embed(vec![query.clone()], "retrieval.query")
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let query_vector = query_vectors
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "no embedding returned"))?;

    // 2. Hybrid search in Qdrant
    let filter_user = if meeting_id.is_some() {
        None
    } else {
        Some(user_id.as_str())
    };
    let search_results = qdrant
        .hybrid_search(query_vector, filter_user, 15)
        .await
        .map_err(|e| ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(results = search_results.len(), "Qdrant search for chat");

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    if search_results.is_empty() {
        tokio::spawn(async move {
            let _ = tx.send(Ok(Event::default().data(
                json!({"type": "answer_chunk", "content": "No relevant transcript content found for your question."}).to_string(),
            ))).await;
            let _ = tx.send(Ok(Event::default().data(
                json!({"type": "done", "sources": []}).to_string(),
            ))).await;
        });
        return Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()).into_response());
    }

    // 3. Rerank via Jina
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
                "meeting_id": result.meeting_id,
                "meeting_title": result.meeting_title,
                "text": result.text,
                "start_ms": result.start_ms,
                "end_ms": result.end_ms,
                "speaker_label": result.speaker_label,
                "relevance_score": score,
            })
        })
        .collect();

    let context_chunks: String = top_results
        .iter()
        .enumerate()
        .map(|(i, (result, _))| {
            let speaker = result.speaker_label.as_deref().unwrap_or("Unknown");
            let timestamp = format_ms(result.start_ms);
            let title = if result.meeting_title.is_empty() { &result.meeting_id } else { &result.meeting_title };
            format!("[Source {}] (Meeting: {}, Speaker: {}, Time: {})\n{}", i + 1, title, speaker, timestamp, result.text)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 4. Stream from Groq in background task
    let groq_api_key = config.groq.api_key.clone().unwrap_or_default();
    let groq_base_url = config.groq.api_base_url.clone();
    let groq_model = config.groq.notes_model.clone();

    tokio::spawn(async move {
        use futures_util::StreamExt;

        let http = reqwest::Client::new();
        let response = http
            .post(format!("{}/openai/v1/chat/completions", groq_base_url))
            .bearer_auth(&groq_api_key)
            .json(&json!({
                "model": groq_model,
                "stream": true,
                "messages": [
                    {
                        "role": "system",
                        "content": "You are a meeting Q&A assistant. Answer the user's question based ONLY on the meeting transcript excerpts provided below.\n\nRules:\n- Only use information explicitly present in the sources. Never make up information.\n- Reference which source number (e.g. [Source 1]) supports your answer.\n- If the answer is not in the provided excerpts, say: \"I couldn't find information about that in the meeting transcripts.\"\n- Be concise and direct. 2-4 sentences unless the question requires more detail.\n- If a speaker is identified, mention them by name."
                    },
                    {
                        "role": "user",
                        "content": format!("## Meeting Transcript Excerpts\n\n{}\n\n## Question\n\n{}", context_chunks, query)
                    }
                ]
            }))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Ok(Event::default().data(
                    json!({"type": "error", "content": e.to_string()}).to_string(),
                ))).await;
                return;
            }
        };

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

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
                    let _ = tx.send(Ok(Event::default().data(
                        json!({"type": "done", "sources": sources_json}).to_string(),
                    ))).await;
                    return;
                }
                if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                    if let Some(content) = parsed
                        .pointer("/choices/0/delta/content")
                        .and_then(Value::as_str)
                    {
                        if !content.is_empty() {
                            let _ = tx.send(Ok(Event::default().data(
                                json!({"type": "answer_chunk", "content": content}).to_string(),
                            ))).await;
                        }
                    }
                }
            }
        }

        let _ = tx.send(Ok(Event::default().data(
            json!({"type": "done", "sources": sources_json}).to_string(),
        ))).await;
    });

    Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()).into_response())
}

fn format_ms(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}
