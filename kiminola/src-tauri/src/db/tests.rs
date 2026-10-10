//! Hardware update validation: seeds a previous-release database and verifies
//! the candidate build migrates it without losing records. The dedicated
//! hardware runner invokes `db::tests::hardware_update_database_record_survives`
//! by path (see `scripts/hardware-update-validation.ps1`).

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use super::library::LibraryLocation;
use super::meeting_store::{self, NewSegment};
use super::{db_path, now_iso};
use crate::db_safety::init_pool;

async fn open_existing_hardware_database(
    path: &std::path::Path,
) -> Result<SqlitePool, sqlx::Error> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
}

async fn verify_previous_database_ready(
    path: &std::path::Path,
    expected_migration_count: usize,
) -> Result<(), String> {
    let pool = open_existing_hardware_database(path)
        .await
        .map_err(|error| format!("open legacy database for readiness: {error}"))?;
    let source = sqlx::migrate!("./migrations");
    let compatible = crate::migrations::compatible(&pool, &source).await?;
    let expected = compatible
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .collect::<Vec<_>>();
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .map_err(|error| format!("read legacy migration history: {error}"))?;
    if expected_migration_count == 0 || expected_migration_count > expected.len() {
        return Err(format!(
            "selected previous release expects {expected_migration_count} migrations, but current source knows {}",
            expected.len()
        ));
    }
    if applied.len() != expected_migration_count {
        return Err(format!(
            "previous database has {} migrations; selected release requires {expected_migration_count}",
            applied.len()
        ));
    }
    for ((version, checksum, success), migration) in applied
        .iter()
        .zip(expected.iter().take(expected_migration_count))
    {
        if *version != migration.version {
            return Err(format!(
                "previous database migration history is not a compatible prefix: found {version}, expected {}",
                migration.version
            ));
        }
        if !success {
            return Err(format!(
                "legacy database migration {} is not marked successful",
                migration.version
            ));
        }
        if checksum.as_slice() != migration.checksum.as_ref() {
            return Err(format!(
                "legacy database migration {} has an incompatible checksum",
                migration.version
            ));
        }
    }

    let integrity: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("run legacy database integrity check: {error}"))?;
    if integrity != "ok" {
        return Err(format!(
            "legacy database integrity check returned {integrity}"
        ));
    }
    for table in [
        "settings",
        "meetings",
        "notes",
        "spaces",
        "transcript_segments",
        "templates",
        "note_drafts",
        "search_index",
    ] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("check legacy table {table}: {error}"))?;
        if !exists {
            return Err(format!("legacy database is missing required table {table}"));
        }
    }
    let personal_spaces: i64 =
        sqlx::query_scalar("SELECT count(*) FROM spaces WHERE name = 'Personal'")
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("query legacy Personal space: {error}"))?;
    pool.close().await;
    if personal_spaces != 1 {
        return Err(format!(
            "legacy database requires exactly one Personal space, found {personal_spaces}"
        ));
    }
    Ok(())
}

async fn seed_hardware_update_record(
    path: &std::path::Path,
    marker: &str,
    note: &str,
) -> Result<(), String> {
    let pool = open_existing_hardware_database(path)
        .await
        .map_err(|error| format!("open previous-version database without migrations: {error}"))?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("start hardware seed transaction: {error}"))?;
    let personal_space_id: i64 =
        sqlx::query_scalar("SELECT id FROM spaces WHERE name = 'Personal' ORDER BY id LIMIT 1")
            .fetch_one(&mut *transaction)
            .await
            .map_err(|error| format!("find previous-version Personal space: {error}"))?;
    let space_result =
        sqlx::query("INSERT INTO spaces(name, parent_id, created_at) VALUES(?, ?, ?)")
            .bind(marker)
            .bind(personal_space_id)
            .bind(now_iso())
            .execute(&mut *transaction)
            .await
            .map_err(|error| format!("insert pre-update library destination: {error}"))?;
    let space_id = space_result.last_insert_rowid();
    let result = sqlx::query(
        "INSERT INTO meetings(title, space_id, created_at, duration_seconds) VALUES(?, ?, ?, 1)",
    )
    .bind(marker)
    .bind(space_id)
    .bind(now_iso())
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("insert pre-update meeting: {error}"))?;
    let meeting_id = result.last_insert_rowid();
    sqlx::query("INSERT INTO notes(meeting_id, raw_markdown, enhanced_markdown, updated_at) VALUES(?, ?, ?, ?)")
        .bind(meeting_id)
        .bind(note)
        .bind(note)
        .bind(now_iso())
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("insert pre-update note: {error}"))?;
    sqlx::query("INSERT INTO transcript_segments(meeting_id, channel, start_ms, end_ms, text) VALUES(?, 'you', 111, 222, ?)")
        .bind(meeting_id)
        .bind(note)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("insert pre-update transcript segment: {error}"))?;
    sqlx::query("INSERT INTO templates(name, prompt, is_builtin) VALUES(?, ?, 0)")
        .bind(marker)
        .bind(format!(
            "Transcript:\n{{transcript}}\nNotes:\n{{notes}}\nFixture: {marker}"
        ))
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("insert pre-update template: {error}"))?;
    sqlx::query("INSERT INTO settings(key, value) VALUES(?, ?)")
        .bind(marker)
        .bind(note)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("insert pre-update setting: {error}"))?;
    let recovery_transcript = vec![NewSegment {
        channel: "you".to_string(),
        text: marker.to_string(),
        start_ms: Some(111),
        end_ms: Some(222),
    }];
    let recovery_transcript_json = serde_json::to_string(&recovery_transcript)
        .map_err(|error| format!("encode pre-update recovery transcript: {error}"))?;
    let recovery_location_json =
        meeting_store::encode_location(Some(&LibraryLocation::Space { id: space_id }))?
            .expect("a supplied recovery location serializes to Some");
    sqlx::query("INSERT INTO note_drafts(title, created_at, updated_at, raw_markdown, meeting_id, recovery_transcript_json, recovery_duration_seconds, recovery_location_json) VALUES(?, ?, ?, ?, ?, ?, 7, ?)")
        .bind(marker)
        .bind(now_iso())
        .bind(now_iso())
        .bind(note)
        .bind(meeting_id)
        .bind(recovery_transcript_json)
        .bind(recovery_location_json)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("insert pre-update recovery draft: {error}"))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit hardware seed transaction: {error}"))?;
    pool.close().await;
    Ok(())
}

async fn verify_hardware_update_record(
    path: &std::path::Path,
    marker: &str,
    note: &str,
) -> Result<(), String> {
    let pool = open_existing_hardware_database(path)
        .await
        .map_err(|error| format!("open candidate-migrated database: {error}"))?;
    verify_current_migration_history(&pool).await?;
    let expected_template_prompt =
        format!("Transcript:\n{{transcript}}\nNotes:\n{{notes}}\nFixture: {marker}");
    let expected_recovery_transcript = vec![NewSegment {
        channel: "you".to_string(),
        text: marker.to_string(),
        start_ms: Some(111),
        end_ms: Some(222),
    }];
    let preserved: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            EXISTS(SELECT 1 FROM meetings m JOIN spaces s ON s.id = m.space_id WHERE m.title = ? AND s.name = ?),
            EXISTS(SELECT 1 FROM meetings m JOIN notes n ON n.meeting_id = m.id WHERE m.title = ? AND n.raw_markdown = ? AND n.enhanced_markdown = ?),
            EXISTS(SELECT 1 FROM meetings m JOIN transcript_segments t ON t.meeting_id = m.id WHERE m.title = ? AND t.start_ms = 111 AND t.end_ms = 222 AND t.text = ?),
            EXISTS(SELECT 1 FROM templates WHERE name = ? AND prompt = ? AND is_builtin = 0),
            EXISTS(SELECT 1 FROM settings WHERE key = ? AND value = ?),
            EXISTS(SELECT 1 FROM note_drafts d JOIN meetings m ON m.id = d.meeting_id JOIN spaces s ON s.id = m.space_id WHERE d.title = ? AND d.raw_markdown = ? AND d.recovery_duration_seconds = 7 AND s.name = ?)"
    )
    .bind(marker).bind(marker)
    .bind(marker).bind(note).bind(note)
    .bind(marker).bind(note)
    .bind(marker).bind(&expected_template_prompt)
    .bind(marker).bind(note)
    .bind(marker).bind(note).bind(marker)
    .fetch_one(&pool)
    .await
    .map_err(|error| format!("query complete pre-update preservation fixture: {error}"))?;
    if preserved != (1, 1, 1, 1, 1, 1) {
        return Err(format!(
            "pre-update preservation fixture is incomplete after migration: {preserved:?}"
        ));
    }
    let destination_id: i64 = sqlx::query_scalar("SELECT id FROM spaces WHERE name = ?")
        .bind(marker)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("find preserved fixture destination: {error}"))?;
    let draft_id: i64 = sqlx::query_scalar("SELECT id FROM note_drafts WHERE title = ?")
        .bind(marker)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("find preserved recovery draft: {error}"))?;
    let draft = meeting_store::get_note_draft(&pool, draft_id).await?;
    if draft.raw_markdown != note
        || draft.recovery_duration_seconds != 7
        || draft.recovery_transcript != expected_recovery_transcript
        || draft.recovery_location != Some(LibraryLocation::Space { id: destination_id })
    {
        return Err("preserved recovery draft is not readable through application types".into());
    }
    pool.close().await;
    Ok(())
}

async fn verify_current_migration_history(pool: &SqlitePool) -> Result<(), String> {
    let source = sqlx::migrate!("./migrations");
    let expected = crate::migrations::compatible(pool, &source).await?;
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("read candidate migration history: {error}"))?;

    for migration in expected
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
    {
        let Some((_, checksum, success)) = applied
            .iter()
            .find(|(version, _, _)| *version == migration.version)
        else {
            return Err(format!(
                "candidate migration history is missing migration {}",
                migration.version
            ));
        };
        if !success {
            return Err(format!(
                "candidate migration {} is not marked successful",
                migration.version
            ));
        }
        if checksum.as_slice() != migration.checksum.as_ref() {
            return Err(format!(
                "candidate migration {} has an incompatible checksum",
                migration.version
            ));
        }
    }
    if let Some((version, _, _)) = applied.iter().find(|(version, _, _)| {
        !expected.iter().any(|migration| {
            !migration.migration_type.is_down_migration() && migration.version == *version
        })
    }) {
        return Err(format!(
            "candidate migration history contains unexpected migration {version}"
        ));
    }
    Ok(())
}

#[tokio::test]
async fn hardware_update_seed_supports_v0_1_2_schema_without_migrating() {
    let directory = std::env::temp_dir().join(format!(
        "kiminola-hardware-seed-regression-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = directory.join("kiminola.db");
    std::fs::create_dir_all(&directory).unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let mut baseline = sqlx::migrate!("./migrations");
    // v0.1.2 shipped migrations 0001 through 0009. Keep this fixture at
    // that published boundary when newer migrations are added.
    baseline.migrations = std::borrow::Cow::Owned(baseline.migrations[..9].to_vec());
    baseline.run(&pool).await.unwrap();
    pool.close().await;

    seed_hardware_update_record(&path, "legacy marker", "legacy note")
        .await
        .unwrap();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let migration_count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    let marker_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM meetings m
         JOIN notes n ON n.meeting_id = m.id
         WHERE m.title = ? AND n.raw_markdown = ?",
    )
    .bind("legacy marker")
    .bind("legacy note")
    .fetch_one(&pool)
    .await
    .unwrap();
    pool.close().await;

    assert_eq!(
        migration_count, 9,
        "seeding must not run migrations newer than the v0.1.2 baseline"
    );
    assert_eq!(marker_count, 1);

    let migrated = init_pool(&path).await.unwrap();
    verify_current_migration_history(&migrated).await.unwrap();
    migrated.close().await;
    verify_hardware_update_record(&path, "legacy marker", "legacy note")
        .await
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn legacy_readiness_requires_the_complete_v0_1_2_schema() {
    let directory = std::env::temp_dir().join(format!(
        "kiminola-legacy-readiness-regression-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = directory.join("kiminola.db");
    std::fs::create_dir_all(&directory).unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let mut incomplete = sqlx::migrate!("./migrations");
    incomplete.migrations = std::borrow::Cow::Owned(incomplete.migrations[..8].to_vec());
    incomplete.run(&pool).await.unwrap();
    pool.close().await;

    let error = verify_previous_database_ready(&path, 9).await.unwrap_err();
    assert!(error.contains("requires 9"), "unexpected error: {error}");

    let pool = open_existing_hardware_database(&path).await.unwrap();
    let mut complete = sqlx::migrate!("./migrations");
    complete.migrations = std::borrow::Cow::Owned(complete.migrations[..9].to_vec());
    complete.run(&pool).await.unwrap();
    pool.close().await;
    verify_previous_database_ready(&path, 9).await.unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
#[ignore = "mutates the provisioned user database on a dedicated hardware runner"]
async fn hardware_update_database_record_survives() {
    assert_eq!(
        std::env::var("KIMINOLA_HARDWARE_RUNNER").as_deref(),
        Ok("1"),
        "refusing to touch the user database outside a dedicated hardware runner"
    );
    let mode = std::env::var("KIMINOLA_HARDWARE_UPDATE_MODE")
        .expect("KIMINOLA_HARDWARE_UPDATE_MODE must be legacy-ready, seed, or verify");
    let path = db_path().expect("resolve the production database path");

    match mode.as_str() {
        "legacy-ready" => {
            let expected_migration_count =
                std::env::var("KIMINOLA_EXPECTED_PREVIOUS_MIGRATION_COUNT")
                    .expect("KIMINOLA_EXPECTED_PREVIOUS_MIGRATION_COUNT must be set")
                    .parse::<usize>()
                    .expect("KIMINOLA_EXPECTED_PREVIOUS_MIGRATION_COUNT must be an integer");
            verify_previous_database_ready(&path, expected_migration_count)
                .await
                .expect("legacy process completed the selected release database schema");
        }
        "seed" => {
            let marker = std::env::var("KIMINOLA_HARDWARE_UPDATE_MARKER")
                .expect("KIMINOLA_HARDWARE_UPDATE_MARKER must identify this upgrade run");
            let note = format!("hardware update fixture {marker}");
            seed_hardware_update_record(&path, &marker, &note)
                .await
                .expect("seed the untouched previous-version database");
        }
        "verify" => {
            let marker = std::env::var("KIMINOLA_HARDWARE_UPDATE_MARKER")
                .expect("KIMINOLA_HARDWARE_UPDATE_MARKER must identify this upgrade run");
            let note = format!("hardware update fixture {marker}");
            verify_hardware_update_record(&path, &marker, &note)
                .await
                .expect("the candidate migrated and preserved the pre-update record");
        }
        _ => panic!("KIMINOLA_HARDWARE_UPDATE_MODE must be legacy-ready, seed, or verify"),
    }
}
