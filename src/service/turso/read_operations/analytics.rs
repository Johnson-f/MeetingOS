use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Utc};
use libsql::params;

use crate::models::{AnalyticsOverview, IntegrationPlaceholder};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn get_analytics_overview_for_user(
        &self,
        user_id: &str,
    ) -> Result<AnalyticsOverview> {
        let conn = self.connection().await?;

        let mut total_rows = conn
            .query(
                r#"
                SELECT COUNT(*)
                FROM meetings m
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                WHERE ma.user_id = ? AND m.deleted_at IS NULL
                "#,
                params![user_id],
            )
            .await?;

        let total_meetings = if let Some(row) = total_rows.next().await? {
            row.get::<i64>(0)?
        } else {
            0
        };

        let mut recorded_rows = conn
            .query(
                r#"
                SELECT COALESCE(SUM(r.duration_seconds), 0)
                FROM recordings r
                INNER JOIN meetings m ON m.id = r.meeting_id
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                WHERE ma.user_id = ? AND m.deleted_at IS NULL
                "#,
                params![user_id],
            )
            .await?;

        let total_recorded_seconds = if let Some(row) = recorded_rows.next().await? {
            row.get::<i64>(0)?
        } else {
            0
        };

        let mut meeting_rows = conn
            .query(
                r#"
                SELECT scheduled_start_at, actual_start_at, created_at
                FROM meetings m
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                WHERE ma.user_id = ? AND m.deleted_at IS NULL
                "#,
                params![user_id],
            )
            .await?;

        let now = Utc::now();
        let week_start = start_of_week(now);
        let week_end = week_start + Duration::days(7);
        let mut meetings_this_week_previous = 0_i64;
        let mut meetings_this_week_upcoming = 0_i64;

        while let Some(row) = meeting_rows.next().await? {
            let effective_at = row
                .get::<Option<String>>(1)?
                .or_else(|| row.get::<Option<String>>(0).ok().flatten())
                .or_else(|| row.get::<Option<String>>(2).ok().flatten());

            let Some(raw_effective_at) = effective_at else {
                continue;
            };

            let Ok(effective_at) = DateTime::parse_from_rfc3339(&raw_effective_at)
                .map(|value| value.with_timezone(&Utc))
            else {
                continue;
            };

            if effective_at < week_start || effective_at >= week_end {
                continue;
            }

            if effective_at < now {
                meetings_this_week_previous += 1;
            } else {
                meetings_this_week_upcoming += 1;
            }
        }

        Ok(AnalyticsOverview {
            total_meetings,
            meetings_this_week_previous,
            meetings_this_week_upcoming,
            recorded_hours: total_recorded_seconds as f64 / 3600.0,
            integrations: IntegrationPlaceholder {
                status: "placeholder".to_owned(),
                label: "Coming soon".to_owned(),
            },
        })
    }
}

fn start_of_week(now: DateTime<Utc>) -> DateTime<Utc> {
    let days_from_monday = i64::from(now.weekday().num_days_from_monday());
    let date = now.date_naive() - Duration::days(days_from_monday);
    let naive = date.and_hms_opt(0, 0, 0).unwrap_or_else(|| now.naive_utc());
    DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
}
