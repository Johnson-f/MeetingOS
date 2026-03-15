use anyhow::{Result, bail};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use sha2::{Digest, Sha256};

const RECALL_JOIN_LEAD_MINUTES: i64 = 10;
const IMMEDIATE_DEDUP_BUCKET_MINUTES: i64 = 15;
const MAX_PAST_MEETING_AGE_HOURS: i64 = 4;

#[derive(Debug, Clone)]
pub struct ResolvedMeetingTiming {
    pub scheduled_start_at: String,
    pub recall_join_at: String,
    pub dedup_time_key: String,
}

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
    dedup_time_key: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(workspace_id.as_bytes());
    hasher.update(b":");
    hasher.update(normalized_meeting_url.as_bytes());
    hasher.update(b":");
    hasher.update(dedup_time_key.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn resolve_meeting_timing(
    meeting_time_mode: &str,
    join_at: Option<&str>,
) -> Result<ResolvedMeetingTiming> {
    let now = Utc::now();

    match meeting_time_mode {
        "future" => {
            let meeting_start = parse_and_validate_meeting_time(
                join_at.ok_or_else(|| {
                    anyhow::anyhow!("join_at is required when meeting_time_mode is future")
                })?,
                now,
            )?;
            let scheduled_join = meeting_start - Duration::minutes(RECALL_JOIN_LEAD_MINUTES);
            let recall_join_at = if scheduled_join > now {
                scheduled_join
            } else {
                now
            };

            Ok(ResolvedMeetingTiming {
                scheduled_start_at: meeting_start.to_rfc3339(),
                recall_join_at: recall_join_at.to_rfc3339(),
                dedup_time_key: meeting_start.to_rfc3339(),
            })
        }
        "starting_now" | "already_started" => {
            let provided_start = join_at
                .map(|value| parse_and_validate_meeting_time(value, now))
                .transpose()?;

            if let Some(start) = provided_start {
                if start > now + Duration::minutes(RECALL_JOIN_LEAD_MINUTES) {
                    bail!(
                        "join_at is too far in the future for {}; use meeting_time_mode=future instead",
                        meeting_time_mode
                    );
                }
            }

            let scheduled_start_at = provided_start.unwrap_or(now);
            Ok(ResolvedMeetingTiming {
                scheduled_start_at: scheduled_start_at.to_rfc3339(),
                recall_join_at: now.to_rfc3339(),
                dedup_time_key: provided_start
                    .map(|start| start.to_rfc3339())
                    .unwrap_or_else(|| immediate_dedup_bucket(now).to_rfc3339()),
            })
        }
        _ => bail!("unsupported meeting_time_mode: {meeting_time_mode}"),
    }
}

fn parse_and_validate_meeting_time(
    meeting_start_at: &str,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>> {
    let meeting_start =
        DateTime::<FixedOffset>::parse_from_rfc3339(meeting_start_at)?.with_timezone(&Utc);

    if meeting_start < now - Duration::hours(MAX_PAST_MEETING_AGE_HOURS) {
        bail!(
            "join_at is too far in the past; meetings older than {} hours are rejected",
            MAX_PAST_MEETING_AGE_HOURS
        );
    }

    Ok(meeting_start)
}

fn immediate_dedup_bucket(now: DateTime<Utc>) -> DateTime<Utc> {
    let bucket_seconds = IMMEDIATE_DEDUP_BUCKET_MINUTES * 60;
    let timestamp = now.timestamp();
    let bucket_start = timestamp - timestamp.rem_euclid(bucket_seconds);
    DateTime::<Utc>::from_timestamp(bucket_start, 0).unwrap_or(now)
}
