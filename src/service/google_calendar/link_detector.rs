use regex::Regex;
use std::sync::LazyLock;

static MEETING_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"https?://(?:meet\.google\.com/[a-z\-]+|(?:[\w-]+\.)?zoom\.us/j/\d+[^\s]*|teams\.microsoft\.com/l/meetup-join/[^\s]+|teams\.live\.com/meet/[^\s]+)"
    ).unwrap()
});

/// Extract meeting URL from a Google Calendar event.
/// Checks conferenceData entry points first, then description, then location.
pub fn extract_meeting_url(event: &serde_json::Value) -> Option<String> {
    // 1. Check conferenceData.entryPoints (most reliable)
    if let Some(entry_points) = event
        .pointer("/conferenceData/entryPoints")
        .and_then(|v| v.as_array())
    {
        for ep in entry_points {
            if let Some(uri) = ep.get("uri").and_then(|v| v.as_str()) {
                if is_meeting_url(uri) {
                    return Some(uri.to_owned());
                }
            }
        }
    }

    // 2. Check hangoutLink (Google Meet shortcut)
    if let Some(hangout_link) = event.get("hangoutLink").and_then(|v| v.as_str()) {
        if is_meeting_url(hangout_link) {
            return Some(hangout_link.to_owned());
        }
    }

    // 3. Check location field
    if let Some(location) = event.get("location").and_then(|v| v.as_str()) {
        if let Some(url) = find_meeting_url_in_text(location) {
            return Some(url);
        }
    }

    // 4. Check description (fallback — regex scan)
    if let Some(description) = event.get("description").and_then(|v| v.as_str()) {
        if let Some(url) = find_meeting_url_in_text(description) {
            return Some(url);
        }
    }

    None
}

fn is_meeting_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("meet.google.com")
        || lower.contains("zoom.us")
        || lower.contains("teams.microsoft.com")
        || lower.contains("teams.live.com")
}

fn find_meeting_url_in_text(text: &str) -> Option<String> {
    MEETING_URL_REGEX.find(text).map(|m| m.as_str().to_owned())
}
