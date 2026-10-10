//! Final-text-only storage. Recovery text never enters this module.
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct HistoryEntry {
    pub id: i64,
    pub text: String,
    pub created_at: String,
}

pub async fn expire(pool: &SqlitePool, now: DateTime<Utc>) -> Result<(), String> {
    sqlx::query("DELETE FROM dictation_history WHERE created_at <= ?")
        .bind((now - Duration::days(30)).to_rfc3339())
        .execute(pool)
        .await
        .map_err(|_| "Could not expire dictation history.".to_string())?;
    Ok(())
}

pub async fn append(pool: &SqlitePool, text: &str, now: DateTime<Utc>) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    expire(pool, now).await?;
    sqlx::query("INSERT INTO dictation_history(text, created_at) VALUES (?, ?)")
        .bind(text)
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .map_err(|_| "Could not save dictation history.".to_string())?;
    Ok(())
}

pub async fn list(pool: &SqlitePool, now: DateTime<Utc>) -> Result<Vec<HistoryEntry>, String> {
    expire(pool, now).await?;
    sqlx::query_as(
        "SELECT id, text, created_at FROM dictation_history ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| "Could not read dictation history.".to_string())
}

pub async fn delete(pool: &SqlitePool, id: Option<i64>) -> Result<(), String> {
    sqlx::query("DELETE FROM dictation_history WHERE ? IS NULL OR id = ?")
        .bind(id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| "Could not delete dictation history.".to_string())?;
    Ok(())
}

pub async fn load_settings<T: serde::de::DeserializeOwned + Default>(
    pool: &SqlitePool,
) -> Result<T, String> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'dictation'")
            .fetch_optional(pool)
            .await
            .map_err(|_| "Could not read dictation settings.".to_string())?;
    value
        .map(|value| {
            serde_json::from_str(&value).map_err(|_| {
                "Invalid dictation settings. Reset them before enabling dictation.".to_string()
            })
        })
        .unwrap_or_else(|| Ok(T::default()))
}

pub async fn save_settings<T: serde::Serialize>(
    pool: &SqlitePool,
    settings: &T,
) -> Result<(), String> {
    let value = serde_json::to_string(settings)
        .map_err(|_| "Could not encode dictation settings.".to_string())?;
    sqlx::query("INSERT INTO settings(key, value) VALUES ('dictation', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(value).execute(pool).await.map_err(|_| "Could not save dictation settings.".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn history_expires_at_thirty_days_and_supports_individual_and_all_deletion() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0013_dictation_history.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        append(&pool, "expired", now - Duration::days(30))
            .await
            .unwrap();
        append(&pool, "retained ' text", now - Duration::days(29))
            .await
            .unwrap();
        append(&pool, "newest", now).await.unwrap();
        let rows = list(&pool, now).await.unwrap();
        assert_eq!(
            rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(),
            vec!["newest", "retained ' text"]
        );
        delete(&pool, Some(rows[0].id)).await.unwrap();
        assert_eq!(list(&pool, now).await.unwrap()[0].text, "retained ' text");
        delete(&pool, None).await.unwrap();
        assert!(list(&pool, now).await.unwrap().is_empty());
    }
}
