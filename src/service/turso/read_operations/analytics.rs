use anyhow::Result;
use chrono::{Datelike, Duration, Utc};
use libsql::params;

use crate::models::{AnalyticsOverview, IntegrationPlaceholder};

use super::super::client::TursoClient;

impl TursoClient {
    pub async fn get_analytics_overview_for_user(
        &self,
        user_id: &str,
    ) -> Result<AnalyticsOverview> {
        let conn = self.connection().await?;

        let now = Utc::now();
        let week_start = start_of_week(now);
        let week_end = week_start + Duration::days(7);
        let now_str = now.to_rfc3339();
        let week_start_str = week_start.to_rfc3339();
        let week_end_str = week_end.to_rfc3339();

        let mut rows = conn
            .query(
                r#"
                SELECT
                    COUNT(*) AS total_meetings,
                    COALESCE((
                        SELECT SUM(r.duration_seconds)
                        FROM recordings r
                        WHERE r.meeting_id IN (
                            SELECT ma2.meeting_id FROM meeting_access ma2
                            INNER JOIN meetings m2 ON m2.id = ma2.meeting_id
                            WHERE ma2.user_id = ?1 AND m2.deleted_at IS NULL
                        )
                    ), 0) AS total_recorded_seconds,
                    SUM(CASE
                        WHEN COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) >= ?2
                         AND COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) < ?4
                         AND COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) < ?3
                        THEN 1 ELSE 0
                    END) AS week_previous,
                    SUM(CASE
                        WHEN COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) >= ?2
                         AND COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) < ?4
                         AND COALESCE(m.actual_start_at, m.scheduled_start_at, m.created_at) >= ?3
                        THEN 1 ELSE 0
                    END) AS week_upcoming
                FROM meetings m
                INNER JOIN meeting_access ma ON ma.meeting_id = m.id
                WHERE ma.user_id = ?1 AND m.deleted_at IS NULL
                "#,
                params![user_id, week_start_str, now_str, week_end_str],
            )
            .await?;

        let (total_meetings, total_recorded_seconds, week_previous, week_upcoming) =
            if let Some(row) = rows.next().await? {
                (
                    row.get::<Option<i64>>(0)?.unwrap_or(0),
                    row.get::<Option<i64>>(1)?.unwrap_or(0),
                    row.get::<Option<i64>>(2)?.unwrap_or(0),
                    row.get::<Option<i64>>(3)?.unwrap_or(0),
                )
            } else {
                (0, 0, 0, 0)
            };

        Ok(AnalyticsOverview {
            total_meetings,
            meetings_this_week_previous: week_previous,
            meetings_this_week_upcoming: week_upcoming,
            recorded_hours: total_recorded_seconds as f64 / 3600.0,
            integrations: IntegrationPlaceholder {
                status: "placeholder".to_owned(),
                label: "Coming soon".to_owned(),
            },
        })
    }
}

fn start_of_week(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    let days_from_monday = i64::from(now.weekday().num_days_from_monday());
    let date = now.date_naive() - Duration::days(days_from_monday);
    let naive = date.and_hms_opt(0, 0, 0).unwrap_or_else(|| now.naive_utc());
    chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
}
