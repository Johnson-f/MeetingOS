use anyhow::Result;
use tracing::info;

use super::RedisClient;

// TTLs in seconds
const MEETINGS_LIST_TTL: u64 = 300; // 5 minutes
const ANALYTICS_TTL: u64 = 300; // 5 minutes
const AUDIO_URL_TTL: u64 = 240; // 4 minutes (presigned URLs expire at 5)
const NOTE_TTL: u64 = 86400; // 1 day
const TRANSCRIPT_TTL: u64 = 86400; // 1 day

// --- Key builders ---

fn meetings_list_key(user_id: &str, limit: usize, offset: usize) -> String {
    format!("user:{}:meetings:{}:{}", user_id, limit, offset)
}

fn analytics_key(user_id: &str) -> String {
    format!("user:{}:analytics", user_id)
}

fn audio_url_key(meeting_id: &str) -> String {
    format!("meeting:{}:audio_url", meeting_id)
}

fn note_key(meeting_id: &str) -> String {
    format!("meeting:{}:note", meeting_id)
}

fn transcript_key(meeting_id: &str) -> String {
    format!("meeting:{}:transcript", meeting_id)
}

impl RedisClient {
    // --- Meetings list ---

    pub async fn get_cached_meetings(&self, user_id: &str, limit: usize, offset: usize) -> Result<Option<String>> {
        let key = meetings_list_key(user_id, limit, offset);
        self.get(&key).await
    }

    pub async fn set_cached_meetings(&self, user_id: &str, limit: usize, offset: usize, json: &str) -> Result<()> {
        let key = meetings_list_key(user_id, limit, offset);
        self.set(&key, json, MEETINGS_LIST_TTL).await
    }

    // --- Analytics ---

    pub async fn get_cached_analytics(&self, user_id: &str) -> Result<Option<String>> {
        let key = analytics_key(user_id);
        self.get(&key).await
    }

    pub async fn set_cached_analytics(&self, user_id: &str, json: &str) -> Result<()> {
        let key = analytics_key(user_id);
        self.set(&key, json, ANALYTICS_TTL).await
    }

    // --- Audio URL (write-once) ---

    pub async fn get_cached_audio_url(&self, meeting_id: &str) -> Result<Option<String>> {
        let key = audio_url_key(meeting_id);
        self.get(&key).await
    }

    pub async fn set_cached_audio_url(&self, meeting_id: &str, url: &str) -> Result<()> {
        let key = audio_url_key(meeting_id);
        self.set(&key, url, AUDIO_URL_TTL).await
    }

    // --- Note (write-once) ---

    pub async fn get_cached_note(&self, meeting_id: &str) -> Result<Option<String>> {
        let key = note_key(meeting_id);
        self.get(&key).await
    }

    pub async fn set_cached_note(&self, meeting_id: &str, json: &str) -> Result<()> {
        let key = note_key(meeting_id);
        self.set(&key, json, NOTE_TTL).await
    }

    // --- Transcript (write-once) ---

    pub async fn get_cached_transcript(&self, meeting_id: &str) -> Result<Option<String>> {
        let key = transcript_key(meeting_id);
        self.get(&key).await
    }

    pub async fn set_cached_transcript(&self, meeting_id: &str, json: &str) -> Result<()> {
        let key = transcript_key(meeting_id);
        self.set(&key, json, TRANSCRIPT_TTL).await
    }

    // --- Invalidation ---

    pub async fn invalidate_user_caches(&self, user_id: &str) {
        // Delete all known meeting list pages (we can't enumerate, so delete common ones)
        for offset in (0..200).step_by(25) {
            for limit in [10, 25, 100] {
                let key = meetings_list_key(user_id, limit, offset);
                let _ = self.del(&key).await;
            }
        }
        let _ = self.del(&analytics_key(user_id)).await;
        info!(user_id = %user_id, "invalidated user caches");
    }
}
