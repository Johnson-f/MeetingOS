# Speaker Diarization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace generic "SPEAKER_00" labels in transcripts with actual participant names by enabling Recall.ai diarization and mapping speaker timelines to Whisper segments.

**Architecture:** Enable diarization in the Recall bot config so Recall captures per-participant speaking timelines. In `fetch_recording_media_job`, extract and store the speaker timeline JSON on the recording row. In `transcribe_recording_job`, after Groq returns segments, load the timeline and re-label each segment's `speaker_label` with the participant name that has the most overlap.

**Tech Stack:** Rust/Axum backend, Recall.ai diarization API, Groq/Whisper transcription, libSQL/Turso.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/service/recall_ai/client.rs` | Modify | Add `diarization` to bot config, add `extract_speaker_timeline` method |
| `src/service/recall_ai/types.rs` | Modify | Add `SpeakerSpan` struct |
| `src/service/turso/schema/tables/mod.rs` | Modify | Add `speaker_timeline_json` column to recordings |
| `src/service/turso/read_operations/processing.rs` | Modify | Store and retrieve speaker timeline JSON |
| `src/service/jobs/handlers.rs` | Modify | Extract timeline in fetch job, re-label segments in transcribe job |

---

## Task 1: Add `SpeakerSpan` type and `extract_speaker_timeline` method

**Files:**
- Modify: `src/service/recall_ai/types.rs`
- Modify: `src/service/recall_ai/client.rs`

- [ ] **Step 1: Add `SpeakerSpan` struct to types.rs**

Add at the end of `src/service/recall_ai/types.rs` (after `RecallParticipantEvent`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSpan {
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
}
```

- [ ] **Step 2: Add `extract_speaker_timeline` method to client.rs**

Add to `impl RecallAiClient` in `src/service/recall_ai/client.rs`, after `extract_participants`:

```rust
/// Extract a flat, sorted list of speaker spans from Recall's diarization data.
/// Each participant's `speaker_timeline` contains `{timestamp, duration}` entries
/// where timestamp and duration are in seconds (f64).
pub fn extract_speaker_timeline(&self, payload: &Value) -> Vec<super::types::SpeakerSpan> {
    let participants = payload
        .get("meeting_participants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut spans: Vec<super::types::SpeakerSpan> = Vec::new();

    for p in &participants {
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_owned();

        let timeline = match p.get("speaker_timeline").and_then(Value::as_array) {
            Some(t) => t,
            None => continue,
        };

        for entry in timeline {
            let timestamp_s = entry
                .get("timestamp")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let duration_s = entry
                .get("duration")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let start_ms = (timestamp_s * 1000.0).round() as i64;
            let end_ms = ((timestamp_s + duration_s) * 1000.0).round() as i64;

            if end_ms > start_ms {
                spans.push(super::types::SpeakerSpan {
                    name: name.clone(),
                    start_ms,
                    end_ms,
                });
            }
        }
    }

    spans.sort_by_key(|s| s.start_ms);
    spans
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 2: Enable diarization in bot config

**Files:**
- Modify: `src/service/recall_ai/client.rs:54-67`

- [ ] **Step 1: Add diarization config to create_bot JSON**

In `src/service/recall_ai/client.rs`, in the `create_bot` method, change the JSON payload from:

```rust
            .json(&json!({
                "meeting_url": payload.meeting_url,
                "bot_name": payload.bot_name,
                "join_at": payload.join_at,
                "metadata": payload.metadata,
                "recording_config": {
                    "audio_mixed_mp3": {},
                },
                "automatic_leave": {
                    "waiting_room_timeout": 300,
                    "noone_joined_timeout": 300,
                    "everyone_left_timeout": 40,
                }
            }))
```

To:

```rust
            .json(&json!({
                "meeting_url": payload.meeting_url,
                "bot_name": payload.bot_name,
                "join_at": payload.join_at,
                "metadata": payload.metadata,
                "recording_config": {
                    "audio_mixed_mp3": {},
                },
                "automatic_leave": {
                    "waiting_room_timeout": 300,
                    "noone_joined_timeout": 300,
                    "everyone_left_timeout": 40,
                },
                "diarization": {
                    "use_separate_streams_when_available": true,
                }
            }))
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 3: Add `speaker_timeline_json` column to recordings table

**Files:**
- Modify: `src/service/turso/schema/tables/mod.rs`

- [ ] **Step 1: Add column to the recordings table schema**

In `src/service/turso/schema/tables/mod.rs`, in the `recordings` CREATE TABLE (around line 93-106), add `speaker_timeline_json TEXT,` after `ended_at TEXT,` (line 101):

From:
```sql
    ended_at TEXT,
    created_at TEXT NOT NULL,
```

To:
```sql
    ended_at TEXT,
    speaker_timeline_json TEXT,
    created_at TEXT NOT NULL,
```

No manual migration needed — the schema auto-migration in `logic.rs` diffs the CREATE TABLE definition against the live DB and runs `ALTER TABLE ADD COLUMN` automatically on startup.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 4: Store and retrieve speaker timeline on recordings

**Files:**
- Modify: `src/service/turso/read_operations/processing.rs`

- [ ] **Step 1: Add `store_speaker_timeline` method**

Add inside `impl TursoClient` in `src/service/turso/read_operations/processing.rs`:

```rust
pub async fn store_speaker_timeline(
    &self,
    recording_id: &str,
    timeline_json: &str,
) -> Result<()> {
    let conn = self.connection().await?;
    conn.execute(
        "UPDATE recordings SET speaker_timeline_json = ?, updated_at = ? WHERE id = ?",
        (timeline_json, now_rfc3339(), recording_id),
    )
    .await?;
    Ok(())
}

pub async fn get_speaker_timeline(&self, recording_id: &str) -> Result<Option<String>> {
    let conn = self.connection().await?;
    let mut rows = conn
        .query(
            "SELECT speaker_timeline_json FROM recordings WHERE id = ? LIMIT 1",
            params![recording_id],
        )
        .await?;

    if let Some(row) = rows.next().await? {
        return Ok(row.get::<Option<String>>(0)?);
    }
    Ok(None)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 5: Extract and store speaker timeline in `fetch_recording_media_job`

**Files:**
- Modify: `src/service/jobs/handlers.rs` (inside `fetch_recording_media_job`)

- [ ] **Step 1: Add speaker timeline extraction after participant extraction**

In `fetch_recording_media_job`, after the participant extraction loop (around line 177, after the `info!` line about extracted participants), add:

```rust
    // Extract and store speaker diarization timeline
    let speaker_timeline = recall.extract_speaker_timeline(&response);
    if !speaker_timeline.is_empty() {
        let timeline_json = serde_json::to_string(&speaker_timeline)
            .unwrap_or_else(|_| "[]".to_owned());
        info!(
            meeting_id = %meeting_id,
            spans = speaker_timeline.len(),
            "extracted speaker diarization timeline"
        );
        // Store after recording is upserted (below), so we do it after upsert_recording_for_bot
        // We'll store it in a moment — save the JSON for now
        let _timeline_json_for_storage = timeline_json;
    }
```

Actually, the recording is upserted later (around line 186-197). So the cleaner approach is to extract the timeline, upsert the recording, then store the timeline. Adjust as follows:

After the `upsert_recording_for_bot` call (around line 197), and before the audio download URL check, add:

```rust
    // Store speaker diarization timeline on the recording
    let speaker_timeline = recall.extract_speaker_timeline(&response);
    if !speaker_timeline.is_empty() {
        if let Ok(timeline_json) = serde_json::to_string(&speaker_timeline) {
            let _ = services
                .turso
                .store_speaker_timeline(&recording.id, &timeline_json)
                .await;
            info!(
                meeting_id = %meeting_id,
                recording_id = %recording.id,
                spans = speaker_timeline.len(),
                "stored speaker diarization timeline"
            );
        }
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 6: Re-label Whisper segments with speaker names in `transcribe_recording_job`

**Files:**
- Modify: `src/service/jobs/handlers.rs` (inside `transcribe_recording_job`)

- [ ] **Step 1: Add the re-labeling logic after Groq transcription, before `replace_transcription`**

In `transcribe_recording_job`, after the `groq.transcribe()` call returns `groq_response` (around line 396), and before the `replace_transcription` call (line 399), add:

```rust
    // Re-label speaker labels using diarization timeline if available
    let mut segments = groq_response.segments;
    if let Ok(Some(timeline_json)) = services.turso.get_speaker_timeline(recording_id).await {
        if let Ok(timeline) = serde_json::from_str::<Vec<crate::service::recall_ai::types::SpeakerSpan>>(&timeline_json) {
            if !timeline.is_empty() {
                let relabeled = relabel_segments_with_speakers(&segments, &timeline);
                segments = relabeled;
                info!(
                    recording_id = %recording_id,
                    "re-labeled transcript segments with speaker names from diarization"
                );
            }
        }
    }
```

Then change the `replace_transcription` call to use `segments` instead of `groq_response.segments`:

From:
```rust
    let transcription = services
        .turso
        .replace_transcription(
            &recording.meeting_id,
            recording_id,
            "groq",
            &services.config.groq.transcription_model,
            groq_response.language.as_deref(),
            &groq_response.text,
            &groq_response.raw_json,
            groq_response.segments,
        )
        .await?;
```

To:
```rust
    let transcription = services
        .turso
        .replace_transcription(
            &recording.meeting_id,
            recording_id,
            "groq",
            &services.config.groq.transcription_model,
            groq_response.language.as_deref(),
            &groq_response.text,
            &groq_response.raw_json,
            segments,
        )
        .await?;
```

- [ ] **Step 2: Add the `relabel_segments_with_speakers` function**

Add at the bottom of `src/service/jobs/handlers.rs` (after the last function, before the file ends):

```rust
/// For each transcript segment, find the speaker span with the most temporal overlap
/// and replace the segment's speaker_label with the speaker's actual name.
fn relabel_segments_with_speakers(
    segments: &[crate::service::recall_ai::types::GroqSegment],
    timeline: &[crate::service::recall_ai::types::SpeakerSpan],
) -> Vec<crate::service::recall_ai::types::GroqSegment> {
    segments
        .iter()
        .map(|seg| {
            let mut best_name: Option<&str> = None;
            let mut best_overlap: i64 = 0;

            for span in timeline {
                let overlap_start = seg.start_ms.max(span.start_ms);
                let overlap_end = seg.end_ms.min(span.end_ms);
                let overlap = (overlap_end - overlap_start).max(0);

                if overlap > best_overlap {
                    best_overlap = overlap;
                    best_name = Some(&span.name);
                }
            }

            let mut relabeled = seg.clone();
            if let Some(name) = best_name {
                relabeled.speaker_label = Some(name.to_owned());
            }
            relabeled
        })
        .collect()
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | tail -5`

---

## Task 7: Verify full build

- [ ] **Step 1: Cargo build**

Run: `cargo build 2>&1 | tail -10`
Expected: build succeeds with no errors
