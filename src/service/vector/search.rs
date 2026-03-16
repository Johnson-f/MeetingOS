use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::service::ServiceRegistry;
use crate::service::recall_ai::GroqClient;

use super::chunker::{TranscriptChunk, TranscriptSegment, chunk_transcript, chunk_full_text};
use crate::service::qdrant_search::client::ChunkPoint;

/// Vectorize a transcript and store in Qdrant
pub async fn vectorize_transcript(
    services: &ServiceRegistry,
    meeting_id: &str,
    user_id: &str,
    meeting_title: &str,
    segments: Vec<TranscriptSegment>,
    full_text: Option<&str>,
) -> Result<()> {
    let jina = services.jina.as_ref().context("Jina is not configured")?;
    let qdrant = services
        .qdrant
        .as_ref()
        .context("Qdrant is not configured")?;

    // Chunk the transcript
    let chunks = if !segments.is_empty() {
        chunk_transcript(&segments, meeting_title)
    } else if let Some(text) = full_text {
        chunk_full_text(text, meeting_title)
    } else {
        return Ok(());
    };

    if chunks.is_empty() {
        info!(meeting_id = %meeting_id, "no chunks generated, skipping vectorization");
        return Ok(());
    }

    info!(meeting_id = %meeting_id, chunk_count = chunks.len(), "chunked transcript");

    // Embed all chunks via Jina
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let embeddings = jina.embed(texts, "retrieval.passage").await?;

    if embeddings.len() != chunks.len() {
        anyhow::bail!(
            "embedding count mismatch: {} embeddings for {} chunks",
            embeddings.len(),
            chunks.len()
        );
    }

    // Build points for Qdrant
    let points: Vec<ChunkPoint> = chunks
        .into_iter()
        .zip(embeddings)
        .map(|(chunk, vector)| ChunkPoint {
            id: uuid::Uuid::new_v4().to_string(),
            meeting_id: meeting_id.to_owned(),
            meeting_title: meeting_title.to_owned(),
            user_id: user_id.to_owned(),
            chunk_index: chunk.chunk_index,
            text: chunk.text,
            start_ms: chunk.start_ms,
            end_ms: chunk.end_ms,
            speaker_label: chunk.speaker_label,
            dense_vector: vector,
        })
        .collect();

    // Delete any existing chunks for this meeting first
    let _ = qdrant.delete_meeting_chunks(meeting_id).await;

    // Upsert new chunks
    qdrant.upsert_chunks(points).await?;

    info!(meeting_id = %meeting_id, "transcript vectorized and stored in Qdrant");
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub answer: String,
    pub sources: Vec<SearchSource>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchSource {
    pub meeting_id: String,
    pub meeting_title: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
    pub relevance_score: f64,
}

/// Perform semantic search over meeting transcripts
pub async fn search_meetings(
    services: &ServiceRegistry,
    query: &str,
    user_id: &str,
    meeting_id: Option<&str>,
) -> Result<SearchResponse> {
    let jina = services.jina.as_ref().context("Jina is not configured")?;
    let qdrant = services
        .qdrant
        .as_ref()
        .context("Qdrant is not configured")?;
    let groq = GroqClient::new(&services.config).context("Groq is not configured")?;

    // 1. Embed the query
    info!(query = %query, "embedding search query");
    let query_vectors = jina.embed(vec![query.to_owned()], "retrieval.query").await?;
    let query_vector = query_vectors
        .into_iter()
        .next()
        .context("no embedding returned for query")?;

    // 2. Hybrid search in Qdrant
    let filter_user = if meeting_id.is_some() {
        None // If searching a specific meeting, don't filter by user (access already verified)
    } else {
        Some(user_id)
    };

    info!(vector_dims = query_vector.len(), user_id = %user_id, meeting_id = ?meeting_id, "starting Qdrant search");
    let search_results = qdrant
        .hybrid_search(query_vector, filter_user, 15)
        .await
        .map_err(|e| {
            warn!(error = %e, "search pipeline failed at Qdrant step");
            e
        })?;

    info!(results = search_results.len(), "Qdrant search returned results");

    if search_results.is_empty() {
        return Ok(SearchResponse {
            answer: "No relevant transcript content found for your question.".to_owned(),
            sources: Vec::new(),
        });
    }

    // 3. Rerank via Jina
    let documents: Vec<String> = search_results.iter().map(|r| r.text.clone()).collect();
    let reranked = jina.rerank(query, documents, 5).await?;

    let top_results: Vec<_> = reranked
        .results
        .iter()
        .filter_map(|r| {
            let original = search_results.get(r.index)?;
            Some((original, r.relevance_score))
        })
        .collect();

    if top_results.is_empty() {
        return Ok(SearchResponse {
            answer: "No relevant transcript content found for your question.".to_owned(),
            sources: Vec::new(),
        });
    }

    // 4. Build context for LLM
    let context_chunks: String = top_results
        .iter()
        .enumerate()
        .map(|(i, (result, _score))| {
            let speaker = result
                .speaker_label
                .as_deref()
                .unwrap_or("Unknown");
            let timestamp = format_ms(result.start_ms);
            format!(
                "[Source {}] (Speaker: {}, Time: {})\n{}",
                i + 1,
                speaker,
                timestamp,
                result.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 5. Send to LLM
    info!("sending context + query to LLM");
    let llm_response = groq
        .http_client()
        .post(format!(
            "{}/openai/v1/chat/completions",
            services.config.groq.api_base_url
        ))
        .bearer_auth(
            services
                .config
                .groq
                .api_key
                .as_deref()
                .unwrap_or_default(),
        )
        .json(&json!({
            "model": services.config.groq.notes_model,
            "messages": [
                {
                    "role": "system",
                    "content": concat!(
                        "You are a meeting Q&A assistant. Answer the user's question based ONLY on the meeting transcript excerpts provided below.\n\n",
                        "Rules:\n",
                        "- Only use information explicitly present in the sources. Never make up information.\n",
                        "- Reference which source number (e.g. [Source 1]) supports your answer.\n",
                        "- If the answer is not in the provided excerpts, say: \"I couldn't find information about that in the meeting transcripts.\"\n",
                        "- Be concise and direct. 2-4 sentences unless the question requires more detail.\n",
                        "- If a speaker is identified, mention them by name."
                    )
                },
                {
                    "role": "user",
                    "content": format!(
                        "## Meeting Transcript Excerpts\n\n{}\n\n## Question\n\n{}",
                        context_chunks, query
                    )
                }
            ]
        }))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;

    let answer = llm_response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("Failed to generate an answer.")
        .to_owned();

    // 6. Build response with sources
    let sources = top_results
        .into_iter()
        .map(|(result, score)| SearchSource {
            meeting_id: result.meeting_id.clone(),
            meeting_title: result.meeting_title.clone(),
            text: result.text.clone(),
            start_ms: result.start_ms,
            end_ms: result.end_ms,
            speaker_label: result.speaker_label.clone(),
            relevance_score: score,
        })
        .collect();

    Ok(SearchResponse { answer, sources })
}

fn format_ms(ms: i64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}
