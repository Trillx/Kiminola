//! Note enhancement templates. Built-in templates are seeded by migrations
//! and are read-only; user templates must keep both `{transcript}` and
//! `{notes}` placeholders so enhancement can fill them.

use sqlx::SqlitePool;

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub is_builtin: i64,
}

pub(crate) async fn list(pool: &SqlitePool) -> Result<Vec<Template>, String> {
    sqlx::query_as::<_, Template>("SELECT id, name, prompt, is_builtin FROM templates ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
}

/// The template a Note enhancement should use: the requested one, or the
/// first template (the default built-in) when none is requested.
pub(crate) async fn resolve(pool: &SqlitePool, template_id: Option<i64>) -> Result<Template, String> {
    let mut templates = list(pool).await?.into_iter();
    match template_id {
        Some(id) => templates
            .find(|template| template.id == id)
            .ok_or_else(|| "template not found".to_string()),
        None => templates
            .next()
            .ok_or_else(|| "no templates available".to_string()),
    }
}

fn validate_prompt(prompt: &str) -> Result<(), String> {
    if !prompt.contains("{transcript}") {
        return Err("prompt must contain {transcript}".into());
    }
    if !prompt.contains("{notes}") {
        return Err("prompt must contain {notes}".into());
    }
    Ok(())
}

pub(crate) async fn create(pool: &SqlitePool, name: &str, prompt: &str) -> Result<Template, String> {
    validate_prompt(prompt)?;
    sqlx::query("INSERT INTO templates (name, prompt, is_builtin) VALUES (?, ?, 0)")
        .bind(name)
        .bind(prompt)
        .execute(pool)
        .await
        .map_err(|e| format!("failed to create template: {e}"))?;
    let id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Template {
        id,
        name: name.to_string(),
        prompt: prompt.to_string(),
        is_builtin: 0,
    })
}

pub(crate) async fn update(pool: &SqlitePool, id: i64, name: &str, prompt: &str) -> Result<(), String> {
    validate_prompt(prompt)?;
    let rows =
        sqlx::query("UPDATE templates SET name = ?, prompt = ? WHERE id = ? AND is_builtin = 0")
            .bind(name)
            .bind(prompt)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
    if rows == 0 {
        return Err("template not found or is built-in".to_string());
    }
    Ok(())
}

pub(crate) async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let rows = sqlx::query("DELETE FROM templates WHERE id = ? AND is_builtin = 0")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("template not found or is built-in".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::TestDb;

    #[tokio::test]
    async fn template_crud_and_built_in_guard() {
        let db = TestDb::new("templates").await;
        let pool = &db.pool;

        // Migrations seed built-in templates; use the first one as the
        // protected row instead of creating a fake one.
        let builtins = list(pool).await.expect("list builtins");
        assert!(
            !builtins.is_empty(),
            "migrations should seed built-in templates"
        );
        let builtin_id = builtins[0].id;

        let custom = create(pool, "My template", "t:{transcript} n:{notes}")
            .await
            .expect("create custom");
        assert_eq!(custom.name, "My template");
        assert_eq!(custom.is_builtin, 0);

        // Missing placeholders rejected.
        assert!(create(pool, "bad", "no placeholders")
            .await
            .unwrap_err()
            .contains("{transcript}"));
        assert!(create(pool, "bad", "{transcript} only")
            .await
            .unwrap_err()
            .contains("{notes}"));

        // Update custom works, update built-in is rejected.
        update(pool, custom.id, "Renamed", "x:{transcript} y:{notes}")
            .await
            .expect("update custom");
        let err = update(pool, builtin_id, "X", "x:{transcript} y:{notes}")
            .await
            .unwrap_err();
        assert!(err.contains("built-in"));

        // Delete custom works, delete built-in is rejected.
        delete(pool, custom.id).await.expect("delete custom");
        let err = delete(pool, builtin_id).await.unwrap_err();
        assert!(err.contains("built-in"));

        let list = list(pool).await.expect("list");
        assert_eq!(list.len(), builtins.len());
        assert!(list.iter().all(|t| t.is_builtin == 1));
    }

    #[tokio::test]
    async fn resolve_picks_the_requested_template_or_the_first() {
        let db = TestDb::new("templates-resolve").await;
        let first = list(&db.pool).await.unwrap().remove(0);
        let custom = create(&db.pool, "Custom", "{transcript} {notes}").await.unwrap();

        assert_eq!(resolve(&db.pool, None).await.unwrap().id, first.id);
        assert_eq!(resolve(&db.pool, Some(custom.id)).await.unwrap().id, custom.id);
        assert_eq!(
            resolve(&db.pool, Some(9_999)).await.unwrap_err(),
            "template not found"
        );
    }
}
