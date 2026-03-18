use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;
use tracing::{info, warn};

use crate::config::ResendConfig;

#[derive(Clone)]
pub struct ResendClient {
    http: Client,
    api_key: String,
    from_email: String,
}

#[derive(Debug, Serialize)]
struct SendEmailRequest<'a> {
    from: &'a str,
    to: &'a [&'a str],
    subject: &'a str,
    html: &'a str,
}

#[derive(Debug)]
pub struct SendEmailResult {
    pub provider_message_id: Option<String>,
}

impl ResendClient {
    pub fn new(config: &ResendConfig) -> Option<Self> {
        let api_key = config.api_key.clone()?;
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            http,
            api_key,
            from_email: config.from_email.clone(),
        })
    }

    pub async fn send_email(&self, to: &str, subject: &str, html: &str) -> Result<SendEmailResult> {
        let body = SendEmailRequest {
            from: &self.from_email,
            to: &[to],
            subject,
            html,
        };

        info!(to = %to, subject = %subject, "sending email via Resend");

        info!(from = %self.from_email, to = %to, "Resend: sending request");

        let response = self
            .http
            .post("https://api.resend.com/emails")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .context("Resend: HTTP request failed")?;

        let status = response.status();
        let response_body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            warn!(status = %status, body = %response_body, to = %to, "Resend API error");
            anyhow::bail!("Resend API returned {status}: {response_body}");
        }

        info!(status = %status, body = %response_body, "Resend: response received");

        let json: serde_json::Value = serde_json::from_str(&response_body).unwrap_or_default();
        let provider_message_id = json.get("id").and_then(|v| v.as_str()).map(str::to_owned);

        Ok(SendEmailResult {
            provider_message_id,
        })
    }
}
