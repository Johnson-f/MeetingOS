use sha2::{Digest, Sha256};

pub(crate) fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub fn normalize_meeting_url(meeting_url: &str) -> String {
    let parsed = match url::Url::parse(meeting_url) {
        Ok(url) => url,
        Err(_) => return meeting_url.trim().to_owned(),
    };

    let mut url = parsed;
    let host = url.host_str().map(|value| value.to_ascii_lowercase());
    let query = url.query_pairs().into_owned().collect::<Vec<_>>();

    if let Some(host) = host {
        let _ = url.set_host(Some(&host));
    }

    url.set_fragment(None);
    if !query.is_empty() {
        let mut pairs = query;
        pairs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in pairs {
            serializer.append_pair(&key, &value);
        }
        url.set_query(Some(&serializer.finish()));
    }

    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if path.is_empty() { "/" } else { &path });
    url.to_string()
}

pub fn platform_from_url(meeting_url: &str) -> String {
    let host = url::Url::parse(meeting_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();

    if host.contains("meet.google.com") {
        "google_meet".to_owned()
    } else if host.contains("zoom.us") {
        "zoom".to_owned()
    } else if host.contains("teams.microsoft.com") || host.contains("teams.live.com") {
        "microsoft_teams".to_owned()
    } else {
        "unknown".to_owned()
    }
}

pub fn build_dedup_key(
    workspace_id: &str,
    normalized_meeting_url: &str,
    scheduled_start_at: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(workspace_id.as_bytes());
    hasher.update(b":");
    hasher.update(normalized_meeting_url.as_bytes());
    hasher.update(b":");
    hasher.update(scheduled_start_at.unwrap_or("adhoc").as_bytes());
    format!("{:x}", hasher.finalize())
}
