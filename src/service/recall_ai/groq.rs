use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::{Client, multipart};
use serde_json::{Value, json};

use crate::config::{AppConfig, GroqConfig};

use super::types::{GeneratedNote, GroqSegment, GroqTranscriptionResponse};

#[derive(Clone)]
pub struct GroqClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl GroqClient {
    pub fn new(config: &AppConfig) -> Option<Self> {
        Self::from_config(&config.groq)
    }

    pub fn from_config(config: &GroqConfig) -> Option<Self> {
        Some(Self {
            http: Client::new(),
            api_key: config.api_key.clone()?,
            base_url: config.api_base_url.trim_end_matches('/').to_owned(),
        })
    }

    pub async fn transcribe(
        &self,
        audio_bytes: Vec<u8>,
        model: &str,
    ) -> Result<GroqTranscriptionResponse> {
        let file_part = multipart::Part::bytes(audio_bytes)
            .file_name("meeting.mp3")
            .mime_str("audio/mpeg")?;

        let form = multipart::Form::new()
            .part("file", file_part)
            .text("model", model.to_owned())
            .text("response_format", "verbose_json".to_owned())
            .text("timestamp_granularities[]", "segment".to_owned());

        let response = self
            .http
            .post(format!("{}/openai/v1/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        let text = response
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let language = response
            .get("language")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let raw_json = response.to_string();
        let segments = response
            .get("segments")
            .and_then(Value::as_array)
            .map(|segments| {
                segments
                    .iter()
                    .enumerate()
                    .map(|(idx, segment)| GroqSegment {
                        seq: idx as i64,
                        speaker_label: segment
                            .get("speaker")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        start_ms: seconds_to_ms(segment.get("start").and_then(Value::as_f64)),
                        end_ms: seconds_to_ms(segment.get("end").and_then(Value::as_f64)),
                        text: segment
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        confidence_json: json!({
                            "avg_logprob": segment.get("avg_logprob").cloned().unwrap_or(Value::Null),
                            "no_speech_prob": segment.get("no_speech_prob").cloned().unwrap_or(Value::Null),
                        })
                        .to_string(),
                        raw_json: segment.to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(GroqTranscriptionResponse {
            text,
            language,
            segments,
            raw_json,
        })
    }

    pub async fn generate_note(&self, model: &str, transcript: &str) -> Result<GeneratedNote> {
        let schema = json!({
            "type": "object",
            "properties": {
                "title": {"type": ["string", "null"]},
                "summary_markdown": {"type": ["string", "null"]},
                "key_points": {"type": "array", "items": {"type": "string"}},
                "decisions": {"type": "array", "items": {"type": "string"}},
                "risks": {"type": "array", "items": {"type": "string"}},
                "action_items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": {"type": "string"},
                            "assignee_name": {"type": ["string", "null"]},
                            "assignee_email": {"type": ["string", "null"]},
                            "due_date": {"type": ["string", "null"]},
                            "priority": {"type": ["string", "null"]},
                            "status": {"type": "string"}
                        },
                        "required": ["description", "status"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["key_points", "decisions", "risks", "action_items"],
            "additionalProperties": false
        });

        let response = self
            .http
            .post(format!("{}/openai/v1/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": model,
                "messages": [
                    {
                        "role": "system",
                        "content": "You are a meeting notes assistant. Produce concise, accurate structured notes from a transcript."
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "Summarize this transcript. Return JSON that matches the provided schema. Transcript:\n\n{}",
                            transcript
                        )
                    }
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "meeting_notes",
                        "schema": schema
                    }
                }
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        let content = response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .context("groq response missing note content")?;

        let mut note: GeneratedNote = serde_json::from_str(content)?;
        if note.summary_markdown.is_none() {
            note.summary_markdown = Some(format!(
                "Generated on {}.\n\n{}",
                Utc::now().format("%Y-%m-%d %H:%M:%SZ"),
                note.key_points.join("\n")
            ));
        }

        Ok(note)
    }
}

fn seconds_to_ms(value: Option<f64>) -> i64 {
    value
        .map(|seconds| (seconds * 1000.0).round() as i64)
        .unwrap_or_default()
}
