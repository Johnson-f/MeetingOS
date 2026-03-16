/// Sentence-based chunker with overlap for meeting transcripts.
///
/// Strategy (based on 2026 RAG research):
/// - Split transcript into sentences
/// - Group into chunks of ~400-512 tokens (~300-400 words)
/// - 10% overlap (last 1-2 sentences carry over to next chunk)
/// - Prepend context (meeting title, speaker) to each chunk

const TARGET_WORDS_PER_CHUNK: usize = 350;
const OVERLAP_SENTENCES: usize = 2;

#[derive(Debug, Clone)]
pub struct TranscriptChunk {
    pub chunk_index: usize,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptSegment {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_label: Option<String>,
}

/// Split transcript segments into overlapping chunks.
/// Each chunk is prepended with context for better retrieval.
pub fn chunk_transcript(
    segments: &[TranscriptSegment],
    meeting_title: &str,
) -> Vec<TranscriptChunk> {
    if segments.is_empty() {
        return Vec::new();
    }

    // First, split all segments into sentences with their metadata
    let sentences = segments_to_sentences(segments);
    if sentences.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut i = 0;

    while i < sentences.len() {
        let mut chunk_sentences = Vec::new();
        let mut word_count = 0;
        let start_i = i;

        // Fill chunk up to target word count
        while i < sentences.len() && word_count < TARGET_WORDS_PER_CHUNK {
            word_count += sentences[i].text.split_whitespace().count();
            chunk_sentences.push(&sentences[i]);
            i += 1;
        }

        if chunk_sentences.is_empty() {
            break;
        }

        let start_ms = chunk_sentences.first().unwrap().start_ms;
        let end_ms = chunk_sentences.last().unwrap().end_ms;

        // Determine primary speaker (most frequent in this chunk)
        let speaker = dominant_speaker(&chunk_sentences);

        // Build context-prepended text
        let speaker_ctx = speaker
            .as_ref()
            .map(|s| format!(" | Speaker: {}", s))
            .unwrap_or_default();
        let context_prefix = format!("[Meeting: {}{}]\n", meeting_title, speaker_ctx);

        let body: String = chunk_sentences
            .iter()
            .map(|s| {
                if let Some(label) = &s.speaker_label {
                    format!("{}: {}", label, s.text)
                } else {
                    s.text.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        chunks.push(TranscriptChunk {
            chunk_index: chunks.len(),
            text: format!("{}{}", context_prefix, body),
            start_ms,
            end_ms,
            speaker_label: speaker,
        });

        // Apply overlap: go back by OVERLAP_SENTENCES for the next chunk
        if i < sentences.len() {
            let overlap_start = if i > OVERLAP_SENTENCES {
                i - OVERLAP_SENTENCES
            } else {
                start_i + 1
            };
            if overlap_start > start_i {
                i = overlap_start;
            }
        }
    }

    chunks
}

/// Chunk from full text when segments aren't available
pub fn chunk_full_text(full_text: &str, meeting_title: &str) -> Vec<TranscriptChunk> {
    let sentences = split_into_sentences(full_text);
    if sentences.is_empty() {
        return Vec::new();
    }

    let fake_segments: Vec<TranscriptSegment> = sentences
        .into_iter()
        .map(|text| TranscriptSegment {
            text,
            start_ms: 0,
            end_ms: 0,
            speaker_label: None,
        })
        .collect();

    chunk_transcript(&fake_segments, meeting_title)
}

#[derive(Debug, Clone)]
struct Sentence {
    text: String,
    start_ms: i64,
    end_ms: i64,
    speaker_label: Option<String>,
}

fn segments_to_sentences(segments: &[TranscriptSegment]) -> Vec<Sentence> {
    let mut sentences = Vec::new();

    for segment in segments {
        let splits = split_into_sentences(&segment.text);
        let count = splits.len();

        for (idx, text) in splits.into_iter().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            // Approximate timestamp distribution within segment
            let duration = segment.end_ms - segment.start_ms;
            let frac_start = if count > 1 {
                idx as f64 / count as f64
            } else {
                0.0
            };
            let frac_end = if count > 1 {
                (idx + 1) as f64 / count as f64
            } else {
                1.0
            };

            sentences.push(Sentence {
                text,
                start_ms: segment.start_ms + (duration as f64 * frac_start) as i64,
                end_ms: segment.start_ms + (duration as f64 * frac_end) as i64,
                speaker_label: segment.speaker_label.clone(),
            });
        }
    }

    sentences
}

fn split_into_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '?' | '!') {
            let trimmed = current.trim().to_owned();
            if !trimmed.is_empty() && trimmed.split_whitespace().count() >= 2 {
                sentences.push(trimmed);
            }
            current.clear();
        }
    }

    // Remaining text that doesn't end with sentence terminator
    let trimmed = current.trim().to_owned();
    if !trimmed.is_empty() && trimmed.split_whitespace().count() >= 2 {
        sentences.push(trimmed);
    }

    sentences
}

fn dominant_speaker(sentences: &[&Sentence]) -> Option<String> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for s in sentences {
        if let Some(label) = &s.speaker_label {
            *counts.entry(label.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(label, _)| label.to_owned())
}
