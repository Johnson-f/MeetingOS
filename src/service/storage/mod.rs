use std::time::Duration;

use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client, config::Region, presigning::PresigningConfig, primitives::ByteStream};
use tracing::warn;

use crate::config::StorageConfig;

const STORAGE_REGION: &str = "auto";

#[derive(Clone)]
pub struct StorageClient {
    client: Client,
    bucket: String,
}

impl StorageClient {
    pub async fn new(config: &StorageConfig) -> Option<Self> {
        let (Some(account_id), Some(bucket), Some(access_key_id), Some(secret_access_key)) = (
            config.account_id.as_deref(),
            config.bucket.as_deref(),
            config.access_key_id.as_deref(),
            config.secret_access_key.as_deref(),
        ) else {
            if config.account_id.is_some()
                || config.bucket.is_some()
                || config.access_key_id.is_some()
                || config.secret_access_key.is_some()
                || config.endpoint.is_some()
            {
                warn!("storage configuration is partial; disabling storage integration");
            }
            return None;
        };

        let endpoint = config
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("https://{account_id}.r2.cloudflarestorage.com"));
        let credentials =
            Credentials::new(access_key_id, secret_access_key, None, None, "static-r2");
        let shared_config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(STORAGE_REGION))
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load()
            .await;
        let timeout_config = aws_config::timeout::TimeoutConfig::builder()
            .read_timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(30))
            .build();
        let sdk_config = aws_sdk_s3::config::Builder::from(&shared_config)
            .force_path_style(true)
            .timeout_config(timeout_config)
            .build();

        Some(Self {
            client: Client::from_conf(sdk_config),
            bucket: bucket.to_owned(),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn audio_object_key(meeting_id: &str, recording_id: &str) -> String {
        format!("meetings/{meeting_id}/recordings/{recording_id}/audio-mixed.mp3")
    }

    pub async fn upload_audio(
        &self,
        key: &str,
        audio_bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(audio_bytes))
            .send()
            .await
            .with_context(|| format!("failed to upload audio object to {key}"))?;

        Ok(())
    }

    pub async fn download_audio(&self, key: &str) -> Result<Vec<u8>> {
        let content_length: Option<i64>;
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .with_context(|| format!("failed to fetch stored audio object {key}"))?;

        content_length = response.content_length();
        tracing::info!(key = %key, content_length = ?content_length, "R2 get_object succeeded, reading body stream");

        let bytes = response
            .body
            .collect()
            .await
            .with_context(|| format!("failed to read stored audio bytes for {key} (content_length={content_length:?})"))?
            .into_bytes()
            .to_vec();

        tracing::info!(key = %key, downloaded_bytes = bytes.len(), "R2 audio download complete");

        Ok(bytes)
    }

    pub async fn presign_audio_get(&self, key: &str, ttl: Duration) -> Result<String> {
        let config = PresigningConfig::expires_in(ttl).context("invalid presign ttl")?;
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .with_context(|| format!("failed to presign audio object {key}"))?;

        Ok(request.uri().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::StorageClient;

    #[test]
    fn audio_object_key_is_deterministic() {
        assert_eq!(
            StorageClient::audio_object_key("meeting-123", "recording-456"),
            "meetings/meeting-123/recordings/recording-456/audio-mixed.mp3"
        );
    }
}
