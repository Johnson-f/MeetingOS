use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use crate::config::GoogleOAuthConfig;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const CALENDAR_API_BASE: &str = "https://www.googleapis.com/calendar/v3";

const SCOPES: &str = "https://www.googleapis.com/auth/calendar.readonly https://www.googleapis.com/auth/calendar.events.readonly";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: String,
    pub primary: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct GoogleCalendarClient {
    http: Client,
    config: GoogleOAuthConfig,
}

impl GoogleCalendarClient {
    pub fn new(config: &GoogleOAuthConfig) -> Option<Self> {
        if config.client_id.is_none() || config.client_secret.is_none() {
            return None;
        }
        info!("Google Calendar client initialized");
        Some(Self {
            http: Client::new(),
            config: config.clone(),
        })
    }

    /// Generate the OAuth consent URL for the user to visit
    pub fn auth_url(&self, state: &str) -> String {
        let client_id = self.config.client_id.as_deref().unwrap_or_default();
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}",
            GOOGLE_AUTH_URL,
            urlencoding::encode(client_id),
            urlencoding::encode(&self.config.redirect_uri),
            urlencoding::encode(SCOPES),
            urlencoding::encode(state),
        )
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(&self, code: &str) -> Result<TokenResponse> {
        let response = self
            .http
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("code", code),
                (
                    "client_id",
                    self.config.client_id.as_deref().unwrap_or_default(),
                ),
                (
                    "client_secret",
                    self.config.client_secret.as_deref().unwrap_or_default(),
                ),
                ("redirect_uri", &self.config.redirect_uri),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| {
                warn!(error = %e, "Google token exchange failed");
                e
            })?
            .json::<TokenResponse>()
            .await
            .context("failed to parse Google token response")?;

        info!("Google OAuth token exchanged successfully");
        Ok(response)
    }

    /// Refresh an expired access token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let response = self
            .http
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("refresh_token", refresh_token),
                (
                    "client_id",
                    self.config.client_id.as_deref().unwrap_or_default(),
                ),
                (
                    "client_secret",
                    self.config.client_secret.as_deref().unwrap_or_default(),
                ),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await
            .context("failed to parse Google refresh token response")?;

        Ok(response)
    }

    /// Revoke a token (on disconnect)
    pub async fn revoke_token(&self, token: &str) -> Result<()> {
        self.http
            .post(GOOGLE_REVOKE_URL)
            .form(&[("token", token)])
            .send()
            .await?;
        Ok(())
    }

    /// List the user's calendars
    pub async fn list_calendars(&self, access_token: &str) -> Result<Vec<CalendarListEntry>> {
        let response = self
            .http
            .get(format!("{}/users/me/calendarList", CALENDAR_API_BASE))
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        let items = response
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let calendars: Vec<CalendarListEntry> = items
            .into_iter()
            .filter_map(|item| {
                Some(CalendarListEntry {
                    id: item.get("id")?.as_str()?.to_owned(),
                    summary: item.get("summary")?.as_str()?.to_owned(),
                    primary: item.get("primary").and_then(|v| v.as_bool()),
                })
            })
            .collect();

        info!(count = calendars.len(), "fetched Google calendars");
        Ok(calendars)
    }

    /// Fetch events from a calendar.
    /// If `sync_token` is provided, does incremental sync.
    /// Otherwise fetches events from `time_min` to `time_max`.
    pub async fn list_events(
        &self,
        access_token: &str,
        calendar_id: &str,
        time_min: Option<&str>,
        time_max: Option<&str>,
        sync_token: Option<&str>,
    ) -> Result<EventsResponse> {
        let mut url = format!(
            "{}/calendars/{}/events?singleEvents=true&orderBy=startTime&maxResults=250",
            CALENDAR_API_BASE,
            urlencoding::encode(calendar_id),
        );

        if let Some(token) = sync_token {
            url.push_str(&format!("&syncToken={}", urlencoding::encode(token)));
        } else {
            if let Some(min) = time_min {
                url.push_str(&format!("&timeMin={}", urlencoding::encode(min)));
            }
            if let Some(max) = time_max {
                url.push_str(&format!("&timeMax={}", urlencoding::encode(max)));
            }
        }

        let response = self.http.get(&url).bearer_auth(access_token).send().await?;

        // If sync token is invalid/expired, Google returns 410 Gone
        if response.status() == 410 {
            bail!("sync token expired, need full resync");
        }

        let body = response.error_for_status()?.json::<Value>().await?;

        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let next_sync_token = body
            .get("nextSyncToken")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        info!(events = items.len(), "fetched Google calendar events");

        Ok(EventsResponse {
            items,
            next_sync_token,
        })
    }

    /// Register a push notification watch on a calendar
    pub async fn watch_calendar(
        &self,
        access_token: &str,
        calendar_id: &str,
        channel_id: &str,
        webhook_url: &str,
    ) -> Result<WatchResponse> {
        let response = self
            .http
            .post(format!(
                "{}/calendars/{}/events/watch",
                CALENDAR_API_BASE,
                urlencoding::encode(calendar_id),
            ))
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "id": channel_id,
                "type": "web_hook",
                "address": webhook_url,
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;

        let resource_id = response
            .get("resourceId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();

        info!(channel_id = %channel_id, resource_id = %resource_id, "Google Calendar watch registered");

        Ok(WatchResponse {
            channel_id: channel_id.to_owned(),
            resource_id,
            expiration: response
                .get("expiration")
                .and_then(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| v.as_u64().map(|n| n.to_string()))
                })
                .unwrap_or_default(),
        })
    }
}

#[derive(Debug)]
pub struct EventsResponse {
    pub items: Vec<Value>,
    pub next_sync_token: Option<String>,
}

#[derive(Debug)]
pub struct WatchResponse {
    pub channel_id: String,
    pub resource_id: String,
    pub expiration: String,
}
