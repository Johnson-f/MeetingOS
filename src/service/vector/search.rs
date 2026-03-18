use anyhow::{Context, Result};
use tracing::info;

use crate::service::ServiceRegistry;

use super::chunker::{TranscriptSegment, chunk_full_text, chunk_transcript};
use crate::service::qdrant_search::ChatQAPoint;
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

/// Vectorize a chat Q&A pair and store in Qdrant
pub async fn vectorize_chat_qa(
    services: &ServiceRegistry,
    thread_id: &str,
    user_id: &str,
    question: &str,
    answer: &str,
) -> Result<()> {
    // Skip if answer is too short or is an error
    if answer.len() < 50 || answer.starts_with("Error:") {
        tracing::info!(thread_id = %thread_id, "skipping chat QA vectorization: answer too short or error");
        return Ok(());
    }

    let jina = services.jina.as_ref().context("jina not configured")?;
    let qdrant = services
        .qdrant_chat
        .as_ref()
        .context("qdrant chat collection not configured")?;

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
