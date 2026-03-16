use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use serde_json::Value;
use tracing::info;

use crate::service::ServiceRegistry;
use crate::service::recall_ai::{build_dedup_key, normalize_meeting_url, platform_from_url};
use crate::service::turso::client::now_rfc3339;
use crate::service::turso::read_operations::MeetingDraft;

use super::link_detector::extract_meeting_url;

/// Sync a single user's Google Calendar.
/// Called after initial connect and by the periodic poll job.
pub async fn sync_user_calendar(
    services: &ServiceRegistry,
    oauth_connection_id: &str,
    access_token: &str,
    user_id: &str,
    workspace_id: &str,
) -> Result<()> {
    let google = services
        .google_calendar
        .as_ref()
        .context("Google Calendar client not configured")?;

    // Get the user's calendars from DB
    let calendars = services
        .turso
        .list_calendar_calendars(oauth_connection_id)
        .await?;

    if calendars.is_empty() {
        info!("no calendars found for oauth connection {}", oauth_connection_id);
        return Ok(());
    }

    let now = Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + Duration::days(30)).to_rfc3339();

    for calendar in &calendars {
        let sync_token = calendar.sync_cursor.as_deref();

        let events_result = google
            .list_events(
                access_token,
                &calendar.provider_calendar_id,
                Some(&time_min),
                Some(&time_max),
                sync_token,
            )
            .await;

        let events_response = match events_result {
            Ok(r) => r,
            Err(e) => {
                if e.to_string().contains("sync token expired") {
                    // Do a full resync
                    info!(calendar_id = %calendar.id, "sync token expired, doing full resync");
                    google
                        .list_events(access_token, &calendar.provider_calendar_id, Some(&time_min), Some(&time_max), None)
                        .await?
                } else {
                    return Err(e);
                }
            }
        };

        let mut created_count = 0;
        let mut updated_count = 0;

        for event in &events_response.items {
            let status = event.get("status").and_then(|v| v.as_str()).unwrap_or("confirmed");
            let event_id = match event.get("id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => continue,
            };

            // Handle cancelled events
            if status == "cancelled" {
                if let Some(existing) = services.turso.get_calendar_event_by_provider_id(&calendar.id, event_id).await? {
                    if let Some(meeting_id) = &existing.meeting_id {
                        services.turso.update_meeting_status(meeting_id, "cancelled", Some("cancelled")).await?;
                        info!(event_id = %event_id, meeting_id = %meeting_id, "cancelled meeting from calendar event");
                    }
                    services.turso.delete_calendar_event(&existing.id).await?;
                }
                continue;
            }

            // Extract meeting URL
            let meeting_url = match extract_meeting_url(event) {
                Some(url) => url,
                None => continue, // No meeting link — skip
            };

            let title = event
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled Meeting")
                .to_owned();

            let starts_at = extract_event_time(event, "start");
            let ends_at = extract_event_time(event, "end");

            // Upsert calendar_event
            let calendar_event_id = services
                .turso
                .upsert_calendar_event(
                    workspace_id,
                    &calendar.id,
                    "google",
                    event_id,
                    &title,
                    Some(&meeting_url),
                    starts_at.as_deref(),
                    ends_at.as_deref(),
                    event.get("organizer").and_then(|v| v.get("email")).and_then(|v| v.as_str()),
                    &event.to_string(),
                )
                .await?;

            // Check if a meeting already exists for this calendar event
            let existing_meeting = services
                .turso
                .get_meeting_for_calendar_event(&calendar_event_id)
                .await?;

            if let Some(meeting) = existing_meeting {
                // Update scheduled time if changed
                if starts_at.as_deref() != meeting.scheduled_start_at.as_deref() {
                    services
                        .turso
                        .update_meeting_fields(
                            &meeting.id,
                            Some(&title),
                            starts_at.as_deref(),
                        )
                        .await?;
                    updated_count += 1;
                }
            } else {
                // Create new meeting
                let normalized = normalize_meeting_url(&meeting_url);
                let platform = platform_from_url(&meeting_url);
                let fallback_time = now_rfc3339();
                let dedup_time = starts_at.as_deref().unwrap_or(&fallback_time);
                let dedup_key = build_dedup_key(workspace_id, &normalized, dedup_time);

                let user_ctx = crate::service::turso::read_operations::UserContext {
                    user_id: user_id.to_owned(),
                    workspace_id: workspace_id.to_owned(),
                };

                let result = services
                    .turso
                    .create_or_get_meeting(
                        &user_ctx,
                        &MeetingDraft {
                            title,
                            source: "google_calendar".to_owned(),
                            original_meeting_url: meeting_url.clone(),
                            normalized_meeting_url: normalized,
                            platform,
                            meeting_time_mode: "future".to_owned(),
                            dedup_key,
                            scheduled_start_at: starts_at.clone().unwrap_or_default(),
                        },
                    )
                    .await?;

                // Link the calendar event to the meeting
                services
                    .turso
                    .link_calendar_event_to_meeting(&calendar_event_id, &result.meeting_id)
                    .await?;

                created_count += 1;
                info!(
                    meeting_id = %result.meeting_id,
                    meeting_url = %meeting_url,
                    "created meeting from calendar event"
                );
            }
        }

        // Store the sync token for incremental sync next time
        if let Some(token) = &events_response.next_sync_token {
            services
                .turso
                .update_calendar_sync_cursor(&calendar.id, token)
                .await?;
        }

        info!(
            calendar = %calendar.provider_calendar_id,
            events = events_response.items.len(),
            created = created_count,
            updated = updated_count,
            "calendar sync complete"
        );
    }

    Ok(())
}

fn extract_event_time(event: &Value, field: &str) -> Option<String> {
    let time_obj = event.get(field)?;
    // Google returns either dateTime (for timed events) or date (for all-day events)
    time_obj
        .get("dateTime")
        .or_else(|| time_obj.get("date"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}
