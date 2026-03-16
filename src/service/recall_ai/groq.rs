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

    pub fn http_client(&self) -> &Client {
        &self.http
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
                        "content": concat!(
                            "You are a professional meeting notes assistant. Your job is to produce structured, accurate notes from a meeting transcript.\n\n",

                            "## Rules\n",
                            "- Only include information that was explicitly stated in the transcript. Never infer, assume, or fabricate.\n",
                            "- If the transcript is too short, low quality, or not a real meeting (e.g. silence, music, a single person testing), still return valid JSON but with empty arrays and a summary that says so.\n",
                            "- Use the speakers' own words and names when possible. Do not rename or anonymize participants.\n\n",

                            "## Field Definitions\n\n",

                            "**title**: A short descriptive title for the meeting (max 10 words). Derive it from the main topic discussed. If unclear, use null.\n\n",

                            "**summary_markdown**: A 2-5 sentence overview of what was discussed, what was decided, and what happens next. Write in past tense. Be specific — mention names, dates, and deliverables if they came up. Do not use bullet points here.\n\n",

                            "**key_points**: The 3-8 most important things discussed. Each should be one sentence. Focus on information someone who missed the meeting would need to know. Do not repeat what is already in decisions or action_items.\n\n",

                            "**decisions**: Explicit decisions that were agreed upon during the meeting. Only include something here if participants clearly agreed to it. Each should be one sentence stating what was decided. If no decisions were made, return an empty array.\n\n",

                            "**risks**: Concerns, blockers, or risks that were raised. Only include if someone explicitly flagged something as a risk, concern, or problem. If none were raised, return an empty array.\n\n",

                            "**action_items**: Tasks that someone committed to doing. Each must have:\n",
                            "- description: What needs to be done (one sentence, start with a verb)\n",
                            "- assignee_name: The person who will do it, using their name as spoken in the transcript. Use null if no one was assigned.\n",
                            "- assignee_email: null (do not guess emails)\n",
                            "- due_date: The deadline if one was explicitly mentioned (ISO 8601 format). null if none was stated.\n",
                            "- priority: \"high\", \"medium\", or \"low\" based on urgency expressed in the conversation. null if unclear.\n",
                            "- status: Always \"open\"\n\n",
                            "Only include action items that were explicitly stated or committed to. Do not infer tasks from general discussion.\n\n",

                            "## Output\n",
                            "Return valid JSON matching the provided schema. No markdown, no code fences, no explanation — just the JSON object."
                        )
                    },
                    {
                        "role": "user",
                        "content": format!(
                            "Here is the meeting transcript. Produce structured notes following the schema.\n\n---\n\n{}",
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
