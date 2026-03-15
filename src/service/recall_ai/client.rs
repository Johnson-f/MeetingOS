use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::config::RecallAiConfig;

use super::{
    types::{RecallCreateBotRequest, RecallCreatedBot, RecallRecordingMedia},
    utils::constant_time_eq,
};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct RecallAiClient {
    http: Client,
    base_url: String,
    api_key: String,
    webhook_secret: Option<String>,
    default_bot_name: String,
}

impl RecallAiClient {
    pub fn new(config: &RecallAiConfig) -> Option<Self> {
        Some(Self {
            http: Client::new(),
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            api_key: config.api_key.clone()?,
            webhook_secret: config.webhook_secret.clone(),
            default_bot_name: config.default_bot_name.clone(),
        })
    }

    pub fn default_bot_name(&self) -> &str {
        &self.default_bot_name
    }

    pub async fn create_bot(
        &self,
        payload: RecallCreateBotRequest<'_>,
    ) -> Result<RecallCreatedBot> {
        let response = self
            .http
            .post(format!("{}/api/v1/bot/", self.base_url))
            .header("Authorization", &self.api_key)
            .json(&json!({
                "meeting_url": payload.meeting_url,
                "bot_name": payload.bot_name,
                "join_at": payload.join_at,
                "metadata": payload.metadata,
                "recording_config": {
                    "audio_mixed_mp3": {},
                }
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        let recall_bot_id = response
            .get("id")
            .and_then(Value::as_str)
            .context("recall create bot response missing id")?
            .to_owned();

        let status = response
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("requested")
            .to_owned();

        Ok(RecallCreatedBot {
            recall_bot_id,
            status,
            raw_json: response,
        })
    }

    pub async fn cancel_scheduled_bot(&self, recall_bot_id: &str) -> Result<()> {
        self.http
            .delete(format!("{}/api/v1/bot/{}/", self.base_url, recall_bot_id))
            .header("Authorization", &self.api_key)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    pub async fn leave_call(&self, recall_bot_id: &str) -> Result<()> {
        self.http
            .post(format!(
                "{}/api/v1/bot/{}/leave_call/",
                self.base_url, recall_bot_id
            ))
            .header("Authorization", &self.api_key)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    pub async fn retrieve_bot(&self, recall_bot_id: &str) -> Result<Value> {
        let response = self
            .http
            .get(format!("{}/api/v1/bot/{}/", self.base_url, recall_bot_id))
            .header("Authorization", &self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        Ok(response)
    }

    pub fn extract_recording_media(&self, payload: &Value) -> RecallRecordingMedia {
        let recordings = payload
            .get("recordings")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let recording = recordings.first().cloned().unwrap_or_else(|| json!({}));
        let shortcuts = recording
            .get("media_shortcuts")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let audio_download_url = shortcuts
            .get("audio_mixed_mp3")
            .or_else(|| shortcuts.get("audio_mixed"))
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("download_url"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        RecallRecordingMedia {
            recording_id: recording
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            duration_seconds: recording.get("duration_seconds").and_then(Value::as_i64),
            started_at: recording
                .get("started_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ended_at: recording
                .get("ended_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
            audio_download_url,
        }
    }

    pub fn verify_webhook(
        &self,
        message_id: &str,
        timestamp: &str,
        signature_header: &str,
        raw_body: &str,
    ) -> Result<bool> {
        let Some(secret) = self.webhook_secret.as_deref() else {
            return Ok(true);
        };

        let secret = secret.strip_prefix("whsec_").unwrap_or(secret);
        let secret_bytes = STANDARD.decode(secret)?;
        let signed_payload = format!("{message_id}.{timestamp}.{raw_body}");
        let mut mac = HmacSha256::new_from_slice(&secret_bytes)
            .map_err(|_| anyhow!("invalid webhook secret"))?;
        mac.update(signed_payload.as_bytes());
        let expected = STANDARD.encode(mac.finalize().into_bytes());

        let valid = signature_header.split_whitespace().any(|candidate| {
            let signature = candidate
                .split_once(',')
                .map(|(_, value)| value)
                .unwrap_or(candidate);
            constant_time_eq(signature.trim(), expected.as_str())
        });

        Ok(valid)
    }
}
