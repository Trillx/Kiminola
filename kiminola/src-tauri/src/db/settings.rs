//! Key-value app settings stored in the SQLite `settings` table.
//!
//! This is the only module that touches `settings` SQL. Owners of a setting
//! (LLM provider config, global shortcut, Meeting presence toggles, onboarding,
//! default Space) keep their key names; secrets never live here (the provider
//! API key stays in the OS keychain).

use sqlx::SqlitePool;

const ONBOARDING_COMPLETE_KEY: &str = "onboarding_complete";

/// The stored value for `key`, or `None` when it was never set or was removed.
pub(crate) async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
}

/// Insert or replace the value for `key`.
pub(crate) async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove `key`. Removing a missing key is not an error.
pub(crate) async fn remove(pool: &SqlitePool, key: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Boolean flag stored as `"1"`/`"0"`. `"true"` (any case) also reads as set;
/// a missing key yields `default`.
pub(crate) async fn get_flag(pool: &SqlitePool, key: &str, default: bool) -> Result<bool, String> {
    Ok(get(pool, key)
        .await?
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(default))
}

pub(crate) async fn set_flag(pool: &SqlitePool, key: &str, value: bool) -> Result<(), String> {
    set(pool, key, if value { "1" } else { "0" }).await
}

/// Onboarding state gate. Only an explicit `"1"` counts as complete.
pub(crate) async fn onboarding_complete(pool: &SqlitePool) -> Result<bool, String> {
    Ok(get(pool, ONBOARDING_COMPLETE_KEY).await?.as_deref() == Some("1"))
}

pub(crate) async fn set_onboarding_complete(
    pool: &SqlitePool,
    complete: bool,
) -> Result<(), String> {
    set_flag(pool, ONBOARDING_COMPLETE_KEY, complete).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::TestDb;

    #[tokio::test]
    async fn values_round_trip_overwrite_and_remove() {
        let db = TestDb::new("settings-roundtrip").await;
        assert_eq!(get(&db.pool, "missing").await.unwrap(), None);

        set(&db.pool, "theme", "dark").await.unwrap();
        set(&db.pool, "theme", "light").await.unwrap();
        assert_eq!(
            get(&db.pool, "theme").await.unwrap().as_deref(),
            Some("light")
        );

        remove(&db.pool, "theme").await.unwrap();
        remove(&db.pool, "theme").await.unwrap();
        assert_eq!(get(&db.pool, "theme").await.unwrap(), None);
    }

    #[tokio::test]
    async fn flags_default_when_missing_and_accept_legacy_true() {
        let db = TestDb::new("settings-flags").await;
        assert!(get_flag(&db.pool, "flag", true).await.unwrap());
        assert!(!get_flag(&db.pool, "flag", false).await.unwrap());

        set_flag(&db.pool, "flag", true).await.unwrap();
        assert!(get_flag(&db.pool, "flag", false).await.unwrap());
        set_flag(&db.pool, "flag", false).await.unwrap();
        assert!(!get_flag(&db.pool, "flag", true).await.unwrap());

        set(&db.pool, "flag", "TRUE").await.unwrap();
        assert!(get_flag(&db.pool, "flag", false).await.unwrap());
    }

    #[tokio::test]
    async fn onboarding_starts_incomplete_and_can_be_toggled() {
        let db = TestDb::new("settings-onboarding").await;
        assert!(!onboarding_complete(&db.pool).await.unwrap());
        set_onboarding_complete(&db.pool, true).await.unwrap();
        assert!(onboarding_complete(&db.pool).await.unwrap());
        set_onboarding_complete(&db.pool, false).await.unwrap();
        assert!(!onboarding_complete(&db.pool).await.unwrap());
    }
}
