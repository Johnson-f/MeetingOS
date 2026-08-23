use anyhow::Result;
use libsql::{Connection, params::IntoParams};
use serde_json::Value;

pub(crate) async fn query_optional_string<P: IntoParams>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<String>> {
    let mut rows = conn.query(sql, params).await?;
    if let Some(row) = rows.next().await? {
        Ok(Some(row.get::<String>(0)?))
    } else {
        Ok(None)
    }
}

pub(crate) fn parse_string_list(value: Option<String>) -> Vec<String> {
    value
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

pub(crate) fn extract_first_string(payload: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut cursor = payload;
        for key in *path {
            cursor = cursor.get(*key)?;
        }
        cursor.as_str().map(str::to_owned)
    })
}

pub(crate) fn extract_first_i64(payload: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths.iter().find_map(|path| {
        let mut cursor = payload;
        for key in *path {
            cursor = cursor.get(*key)?;
        }
        cursor.as_i64()
    })
}
