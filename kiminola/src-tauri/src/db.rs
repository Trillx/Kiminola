//! SQLite persistence (SPEC.md §6): meetings, transcript segments, notes,
//! spaces, settings. Single bundled database under
//! `%LOCALAPPDATA%\Kiminola\data\kiminola.db`, migrated on first use.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tauri::{Manager, State};
use tokio::sync::OnceCell;

/// Pool behind a `OnceCell` so app setup never blocks on file IO; the first
/// command (or the launch-time warm-up in `setup`) opens and migrates the DB.
pub struct DbState {
    pub(crate) pool: Arc<OnceCell<SqlitePool>>,
}

/// `%LOCALAPPDATA%\Kiminola\data\kiminola.db`, falling back to a `data/`
/// directory next to the executable (portable installs).
fn db_path() -> Result<PathBuf, String> {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(local_app_data)
            .join("Kiminola")
            .join("data")
            .join("kiminola.db"));
    }
    let exe = std::env::current_exe().map_err(|e| format!("no current exe path: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "exe has no parent directory".to_string())?;
    Ok(dir.join("data").join("kiminola.db"))
}

async fn init_pool(path: &Path) -> Result<SqlitePool, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create data dir: {e}"))?;
    }
    // One connection: a single writer is all this app has, and it sidesteps
    // SQLite "database is locked" errors on Windows.
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| format!("failed to open database: {e}"))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| format!("migration failed: {e}"))?;
    Ok(pool)
}

pub(crate) async fn ensure_pool(cell: &OnceCell<SqlitePool>) -> Result<SqlitePool, String> {
    cell.get_or_try_init(|| async {
        let path = db_path()?;
        let pool = init_pool(&path).await?;
        eprintln!("[db] opened {}", path.display());
        Ok(pool)
    })
    .await
    .cloned()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/* ---------- domain types ---------- */

#[derive(serde::Deserialize)]
pub struct NewSegment {
    pub channel: String,
    pub text: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct MeetingSummary {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub duration_seconds: i64,
    pub space_name: Option<String>,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct SegmentOut {
    pub id: i64,
    pub channel: String,
    pub text: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct MeetingDetail {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub duration_seconds: i64,
    pub space_name: Option<String>,
    pub notepad: String,
    pub enhanced_markdown: Option<String>,
    pub transcript: Vec<SegmentOut>,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub is_builtin: i64,
}

#[derive(serde::Serialize)]
pub struct SpaceMeetingRef {
    pub id: i64,
    pub title: String,
}

#[derive(serde::Serialize)]
pub struct SpaceOut {
    pub id: i64,
    pub name: String,
    pub meetings: Vec<SpaceMeetingRef>,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct NoteDraftSummary {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct NoteDraftDetail {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub raw_markdown: String,
    pub meeting_id: Option<i64>,
}

async fn is_onboarding_complete_impl(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'onboarding_complete'")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v == "1").unwrap_or(false))
}

async fn set_onboarding_complete_impl(
    pool: &SqlitePool,
    complete: bool,
) -> Result<(), sqlx::Error> {
    let value = if complete { "1" } else { "0" };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('onboarding_complete', ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn is_onboarding_complete(state: State<'_, DbState>) -> Result<bool, String> {
    let pool = ensure_pool(&state.pool).await?;
    is_onboarding_complete_impl(&pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_onboarding_complete(
    state: State<'_, DbState>,
    complete: bool,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    set_onboarding_complete_impl(&pool, complete)
        .await
        .map_err(|e| e.to_string())
}

/* ---------- core logic (testable without Tauri state) ---------- */

#[allow(dead_code)]
async fn save_meeting_impl(
    pool: &SqlitePool,
    title: &str,
    duration_seconds: i64,
    notepad: &str,
    segments: &[NewSegment],
) -> Result<i64, String> {
    save_meeting_with_draft_impl(pool, title, duration_seconds, notepad, segments, None).await
}

pub(crate) async fn save_meeting_with_draft_impl(
    pool: &SqlitePool,
    title: &str,
    duration_seconds: i64,
    notepad: &str,
    segments: &[NewSegment],
    note_draft_id: Option<i64>,
) -> Result<i64, String> {
    // File under the default (first) space until space management ships.
    let space_id: Option<i64> = sqlx::query_scalar("SELECT id FROM spaces ORDER BY id LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let meeting_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO meetings (title, space_id, created_at, duration_seconds)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(title)
    .bind(space_id)
    .bind(now_iso())
    .bind(duration_seconds)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    for segment in segments {
        sqlx::query(
            "INSERT INTO transcript_segments (meeting_id, channel, start_ms, end_ms, text)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(&segment.channel)
        .bind(segment.start_ms)
        .bind(segment.end_ms)
        .bind(&segment.text)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    let draft_markdown = if let Some(draft_id) = note_draft_id {
        sqlx::query_scalar::<_, String>(
            "SELECT raw_markdown FROM note_drafts
             WHERE id = ? AND meeting_id IS NULL",
        )
        .bind(draft_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "note draft not found or already attached".to_string())?
    } else {
        String::new()
    };
    let saved_notepad = if note_draft_id.is_some() && notepad.is_empty() {
        &draft_markdown
    } else {
        notepad
    };

    sqlx::query("INSERT INTO notes (meeting_id, raw_markdown, updated_at) VALUES (?, ?, ?)")
        .bind(meeting_id)
        .bind(saved_notepad)
        .bind(now_iso())
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(draft_id) = note_draft_id {
        sqlx::query("UPDATE note_drafts SET meeting_id = ?, updated_at = ? WHERE id = ?")
            .bind(meeting_id)
            .bind(now_iso())
            .bind(draft_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(meeting_id)
}

async fn list_meetings_impl(pool: &SqlitePool) -> Result<Vec<MeetingSummary>, String> {
    sqlx::query_as::<_, MeetingSummary>(
        "SELECT m.id, m.title, m.created_at, m.duration_seconds, s.name AS space_name
         FROM meetings m LEFT JOIN spaces s ON s.id = m.space_id
         ORDER BY m.created_at DESC, m.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn get_meeting_impl(pool: &SqlitePool, id: i64) -> Result<MeetingDetail, String> {
    let meeting = sqlx::query_as::<_, MeetingSummary>(
        "SELECT m.id, m.title, m.created_at, m.duration_seconds, s.name AS space_name
         FROM meetings m LEFT JOIN spaces s ON s.id = m.space_id
         WHERE m.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "meeting not found".to_string())?;

    let transcript = sqlx::query_as::<_, SegmentOut>(
        "SELECT id, channel, text, start_ms, end_ms
         FROM transcript_segments WHERE meeting_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let (notepad, enhanced_markdown): (String, Option<String>) =
        sqlx::query_as("SELECT raw_markdown, enhanced_markdown FROM notes WHERE meeting_id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or((String::new(), None));

    Ok(MeetingDetail {
        id: meeting.id,
        title: meeting.title,
        created_at: meeting.created_at,
        duration_seconds: meeting.duration_seconds,
        space_name: meeting.space_name,
        notepad,
        enhanced_markdown,
        transcript,
    })
}

async fn create_space_impl(pool: &SqlitePool, name: &str) -> Result<i64, String> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO spaces (name, created_at) VALUES (?, ?) RETURNING id",
    )
    .bind(name)
    .bind(now_iso())
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

fn note_draft_title() -> String {
    format!(
        "Note draft · {}",
        chrono::Local::now().format("%b %-d, %Y, %-I:%M %p")
    )
}

pub(crate) async fn create_note_draft_impl(pool: &SqlitePool) -> Result<i64, String> {
    let now = now_iso();
    sqlx::query(
        "INSERT INTO note_drafts (title, created_at, updated_at, raw_markdown)
         VALUES (?, ?, ?, '')",
    )
    .bind(note_draft_title())
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| format!("failed to create note draft: {e}"))?;
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) async fn list_note_drafts_impl(
    pool: &SqlitePool,
) -> Result<Vec<NoteDraftSummary>, String> {
    sqlx::query_as::<_, NoteDraftSummary>(
        "SELECT id, title, created_at, updated_at
         FROM note_drafts
         WHERE meeting_id IS NULL
         ORDER BY updated_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn get_note_draft_impl(
    pool: &SqlitePool,
    id: i64,
) -> Result<NoteDraftDetail, String> {
    sqlx::query_as::<_, NoteDraftDetail>(
        "SELECT id, title, created_at, updated_at, raw_markdown, meeting_id
         FROM note_drafts
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "note draft not found".to_string())
}

pub(crate) async fn update_note_draft_impl(
    pool: &SqlitePool,
    id: i64,
    raw_markdown: &str,
) -> Result<(), String> {
    let rows = sqlx::query(
        "UPDATE note_drafts SET raw_markdown = ?, updated_at = ?
         WHERE id = ? AND meeting_id IS NULL",
    )
    .bind(raw_markdown)
    .bind(now_iso())
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();
    if rows == 0 {
        return Err("note draft not found or already attached".to_string());
    }
    Ok(())
}

pub(crate) async fn delete_note_draft_impl(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let rows = sqlx::query("DELETE FROM note_drafts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("note draft not found".to_string());
    }
    Ok(())
}

async fn list_spaces_impl(pool: &SqlitePool) -> Result<Vec<SpaceOut>, String> {
    let spaces = sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM spaces ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(spaces.len());
    for (id, name) in spaces {
        let meetings = sqlx::query_as::<_, (i64, String)>(
            "SELECT id, title FROM meetings WHERE space_id = ?
             ORDER BY created_at DESC, id DESC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        out.push(SpaceOut {
            id,
            name,
            meetings: meetings
                .into_iter()
                .map(|(id, title)| SpaceMeetingRef { id, title })
                .collect(),
        });
    }
    Ok(out)
}

async fn update_meeting_title_impl(
    pool: &SqlitePool,
    meeting_id: i64,
    title: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE meetings SET title = ? WHERE id = ?")
        .bind(title)
        .bind(meeting_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn update_notes_impl(
    pool: &SqlitePool,
    meeting_id: i64,
    raw_markdown: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes (meeting_id, raw_markdown, updated_at) VALUES (?, ?, ?)
         ON CONFLICT (meeting_id) DO UPDATE SET
           raw_markdown = excluded.raw_markdown,
           updated_at = excluded.updated_at",
    )
    .bind(meeting_id)
    .bind(raw_markdown)
    .bind(now_iso())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn list_templates_impl(pool: &SqlitePool) -> Result<Vec<Template>, String> {
    sqlx::query_as::<_, Template>("SELECT id, name, prompt, is_builtin FROM templates ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
}

fn validate_template_prompt(prompt: &str) -> Result<(), String> {
    if !prompt.contains("{transcript}") {
        return Err("prompt must contain {transcript}".into());
    }
    if !prompt.contains("{notes}") {
        return Err("prompt must contain {notes}".into());
    }
    Ok(())
}

pub(crate) async fn create_template_impl(
    pool: &SqlitePool,
    name: &str,
    prompt: &str,
) -> Result<Template, String> {
    validate_template_prompt(prompt)?;
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

pub(crate) async fn update_template_impl(
    pool: &SqlitePool,
    id: i64,
    name: &str,
    prompt: &str,
) -> Result<(), String> {
    validate_template_prompt(prompt)?;
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

pub(crate) async fn delete_template_impl(pool: &SqlitePool, id: i64) -> Result<(), String> {
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

fn sanitize_search_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("{}*", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) async fn search_meetings_impl(
    pool: &SqlitePool,
    query: &str,
) -> Result<Vec<MeetingSummary>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let safe = sanitize_search_query(trimmed);
    if safe.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, MeetingSummary>(
        "SELECT m.id, m.title, m.created_at, m.duration_seconds, s.name AS space_name
         FROM search_index
         JOIN meetings m ON m.id = search_index.rowid
         LEFT JOIN spaces s ON s.id = m.space_id
         WHERE search_index MATCH ?
         ORDER BY rank DESC
         LIMIT 50",
    )
    .bind(safe)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn update_enhanced_notes_impl(
    pool: &SqlitePool,
    meeting_id: i64,
    enhanced_markdown: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes (meeting_id, raw_markdown, enhanced_markdown, updated_at)
         VALUES (?, '', ?, ?)
         ON CONFLICT (meeting_id) DO UPDATE SET
           enhanced_markdown = excluded.enhanced_markdown,
           updated_at = excluded.updated_at",
    )
    .bind(meeting_id)
    .bind(enhanced_markdown)
    .bind(now_iso())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn update_segment_text_impl(
    pool: &SqlitePool,
    segment_id: i64,
    text: &str,
) -> Result<(), String> {
    let rows = sqlx::query("UPDATE transcript_segments SET text = ? WHERE id = ?")
        .bind(text)
        .bind(segment_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("segment not found".to_string());
    }
    Ok(())
}

pub(crate) async fn delete_segment_impl(pool: &SqlitePool, segment_id: i64) -> Result<(), String> {
    let rows = sqlx::query("DELETE FROM transcript_segments WHERE id = ?")
        .bind(segment_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("segment not found".to_string());
    }
    Ok(())
}

/* ---------- Tauri commands ---------- */

#[tauri::command]
pub async fn save_meeting(
    state: State<'_, DbState>,
    title: String,
    duration_seconds: i64,
    notepad: String,
    segments: Vec<NewSegment>,
    note_draft_id: Option<i64>,
) -> Result<i64, String> {
    let pool = ensure_pool(&state.pool).await?;
    save_meeting_with_draft_impl(
        &pool,
        &title,
        duration_seconds,
        &notepad,
        &segments,
        note_draft_id,
    )
    .await
}

#[tauri::command]
pub async fn list_meetings(state: State<'_, DbState>) -> Result<Vec<MeetingSummary>, String> {
    let pool = ensure_pool(&state.pool).await?;
    list_meetings_impl(&pool).await
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, DbState>, id: i64) -> Result<MeetingDetail, String> {
    let pool = ensure_pool(&state.pool).await?;
    get_meeting_impl(&pool, id).await
}

#[tauri::command]
pub async fn create_space(state: State<'_, DbState>, name: String) -> Result<i64, String> {
    let pool = ensure_pool(&state.pool).await?;
    create_space_impl(&pool, &name).await
}

#[tauri::command]
pub async fn create_note_draft(state: State<'_, DbState>) -> Result<i64, String> {
    let pool = ensure_pool(&state.pool).await?;
    create_note_draft_impl(&pool).await
}

#[tauri::command]
pub async fn list_note_drafts(state: State<'_, DbState>) -> Result<Vec<NoteDraftSummary>, String> {
    let pool = ensure_pool(&state.pool).await?;
    list_note_drafts_impl(&pool).await
}

#[tauri::command]
pub async fn get_note_draft(state: State<'_, DbState>, id: i64) -> Result<NoteDraftDetail, String> {
    let pool = ensure_pool(&state.pool).await?;
    get_note_draft_impl(&pool, id).await
}

#[tauri::command]
pub async fn update_note_draft(
    state: State<'_, DbState>,
    id: i64,
    raw_markdown: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    update_note_draft_impl(&pool, id, &raw_markdown).await
}

#[tauri::command]
pub async fn delete_note_draft(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    delete_note_draft_impl(&pool, id).await
}

#[tauri::command]
pub async fn list_spaces(state: State<'_, DbState>) -> Result<Vec<SpaceOut>, String> {
    let pool = ensure_pool(&state.pool).await?;
    list_spaces_impl(&pool).await
}

#[tauri::command]
pub async fn rename_meeting(
    state: State<'_, DbState>,
    meeting_id: i64,
    title: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    update_meeting_title_impl(&pool, meeting_id, &title).await
}

#[tauri::command]
pub async fn update_notes(
    state: State<'_, DbState>,
    meeting_id: i64,
    raw_markdown: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    update_notes_impl(&pool, meeting_id, &raw_markdown).await
}

#[tauri::command]
pub async fn update_segment_text(
    state: State<'_, DbState>,
    segment_id: i64,
    text: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    update_segment_text_impl(&pool, segment_id, &text).await
}

#[tauri::command]
pub async fn delete_segment(state: State<'_, DbState>, segment_id: i64) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    delete_segment_impl(&pool, segment_id).await
}

#[tauri::command]
pub async fn list_templates(state: State<'_, DbState>) -> Result<Vec<Template>, String> {
    let pool = ensure_pool(&state.pool).await?;
    list_templates_impl(&pool).await
}

#[tauri::command]
pub async fn create_template(
    state: State<'_, DbState>,
    name: String,
    prompt: String,
) -> Result<Template, String> {
    let pool = ensure_pool(&state.pool).await?;
    create_template_impl(&pool, &name, &prompt).await
}

#[tauri::command]
pub async fn update_template(
    state: State<'_, DbState>,
    id: i64,
    name: String,
    prompt: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    update_template_impl(&pool, id, &name, &prompt).await
}

#[tauri::command]
pub async fn delete_template(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    delete_template_impl(&pool, id).await
}

#[tauri::command]
pub async fn search_meetings(
    state: State<'_, DbState>,
    query: String,
) -> Result<Vec<MeetingSummary>, String> {
    let pool = ensure_pool(&state.pool).await?;
    search_meetings_impl(&pool, &query).await
}

/// Helper used by `lib.rs` to install DB state and warm the pool at launch.
pub fn setup(app: &mut tauri::App) {
    let state = DbState {
        pool: Arc::new(OnceCell::new()),
    };
    let cell = Arc::clone(&state.pool);
    app.manage(state);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = ensure_pool(&cell).await {
            eprintln!("[db] warm-up failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool(name: &str) -> (SqlitePool, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "kiminola-dbtest-{}-{}.db",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_file(&path);
        let pool = init_pool(&path).await.expect("init test pool");
        (pool, path)
    }

    #[tokio::test]
    async fn save_list_get_roundtrip() {
        let (pool, path) = test_pool("roundtrip").await;

        let segments = vec![
            NewSegment {
                channel: "others".into(),
                text: "hello there".into(),
                start_ms: Some(100),
                end_ms: Some(1_200),
            },
            NewSegment {
                channel: "you".into(),
                text: "hi".into(),
                start_ms: Some(1_250),
                end_ms: Some(1_600),
            },
        ];
        let id = save_meeting_impl(&pool, "Test meeting", 95, "my notes", &segments)
            .await
            .expect("save");

        let list = list_meetings_impl(&pool).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Test meeting");
        assert_eq!(list[0].space_name.as_deref(), Some("Personal"));

        let detail = get_meeting_impl(&pool, id).await.expect("get");
        assert_eq!(detail.duration_seconds, 95);
        assert_eq!(detail.notepad, "my notes");
        assert_eq!(detail.transcript.len(), 2);
        assert_eq!(detail.transcript[0].channel, "others");
        assert_eq!(detail.transcript[0].start_ms, Some(100));
        assert_eq!(detail.transcript[0].end_ms, Some(1_200));

        update_notes_impl(&pool, id, "edited notes")
            .await
            .expect("update notes");
        let detail = get_meeting_impl(&pool, id).await.expect("get again");
        assert_eq!(detail.notepad, "edited notes");

        // A second save must not duplicate the notes row (UNIQUE upsert).
        let id2 = save_meeting_impl(&pool, "Second", 10, "", &[])
            .await
            .expect("save 2");
        assert_ne!(id, id2);
        let list = list_meetings_impl(&pool).await.expect("list 2");
        assert_eq!(list.len(), 2);

        let spaces = list_spaces_impl(&pool).await.expect("spaces");
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].meetings.len(), 2);

        drop(pool);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn missing_meeting_errors() {
        let (pool, path) = test_pool("missing").await;
        let err = get_meeting_impl(&pool, 999).await.unwrap_err();
        assert_eq!(err, "meeting not found");
        drop(pool);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn note_draft_roundtrip_and_meeting_attach() {
        let (pool, path) = test_pool("note-draft").await;
        let draft_id = create_note_draft_impl(&pool).await.expect("create draft");

        update_note_draft_impl(&pool, draft_id, "follow up with the team")
            .await
            .expect("update draft");
        let draft = get_note_draft_impl(&pool, draft_id)
            .await
            .expect("get draft");
        assert_eq!(draft.raw_markdown, "follow up with the team");
        assert!(draft.meeting_id.is_none());

        let meeting_id =
            save_meeting_with_draft_impl(&pool, "Follow-up meeting", 60, "", &[], Some(draft_id))
                .await
                .expect("save meeting with draft");
        let meeting = get_meeting_impl(&pool, meeting_id)
            .await
            .expect("get meeting");
        assert_eq!(meeting.notepad, "follow up with the team");
        assert!(list_note_drafts_impl(&pool)
            .await
            .expect("list drafts")
            .is_empty());
        assert_eq!(
            get_note_draft_impl(&pool, draft_id)
                .await
                .expect("get attached draft")
                .meeting_id,
            Some(meeting_id)
        );

        drop(pool);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn template_crud_and_built_in_guard() {
        let (pool, path) = test_pool("templates").await;

        // Migrations seed built-in templates; use the first one as the
        // protected row instead of creating a fake one.
        let builtins = list_templates_impl(&pool).await.expect("list builtins");
        assert!(
            !builtins.is_empty(),
            "migrations should seed built-in templates"
        );
        let builtin_id = builtins[0].id;

        let custom = create_template_impl(&pool, "My template", "t:{transcript} n:{notes}")
            .await
            .expect("create custom");
        assert_eq!(custom.name, "My template");
        assert_eq!(custom.is_builtin, 0);

        // Missing placeholders rejected.
        assert!(create_template_impl(&pool, "bad", "no placeholders")
            .await
            .unwrap_err()
            .contains("{transcript}"));

        // Update custom works, update built-in is rejected.
        update_template_impl(&pool, custom.id, "Renamed", "x:{transcript} y:{notes}")
            .await
            .expect("update custom");
        let err = update_template_impl(&pool, builtin_id, "X", "x:{transcript} y:{notes}")
            .await
            .unwrap_err();
        assert!(err.contains("built-in"));

        // Delete custom works, delete built-in is rejected.
        delete_template_impl(&pool, custom.id)
            .await
            .expect("delete custom");
        let err = delete_template_impl(&pool, builtin_id).await.unwrap_err();
        assert!(err.contains("built-in"));

        let list = list_templates_impl(&pool).await.expect("list");
        assert_eq!(list.len(), builtins.len());
        assert!(list.iter().all(|t| t.is_builtin == 1));

        drop(pool);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn search_indexes_titles_notes_and_transcript() {
        let (pool, path) = test_pool("search").await;

        let id1 = save_meeting_impl(
            &pool,
            "CLI strategy meeting",
            60,
            "discussed CLI and VPS",
            &[NewSegment {
                channel: "you".into(),
                text: "we need a CLI".into(),
                start_ms: None,
                end_ms: None,
            }],
        )
        .await
        .expect("save m1");

        let id2 = save_meeting_impl(
            &pool,
            "Design sync",
            60,
            "colors and buttons",
            &[NewSegment {
                channel: "others".into(),
                text: "the navbar needs work".into(),
                start_ms: None,
                end_ms: None,
            }],
        )
        .await
        .expect("save m2");

        // Titles
        let r = search_meetings_impl(&pool, "CLI")
            .await
            .expect("search title");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, id1);

        // Notes
        let r = search_meetings_impl(&pool, "VPS")
            .await
            .expect("search notes");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, id1);

        // Transcript
        let r = search_meetings_impl(&pool, "navbar")
            .await
            .expect("search transcript");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, id2);

        // Prefix match
        let r = search_meetings_impl(&pool, "strat")
            .await
            .expect("search prefix");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, id1);

        drop(pool);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn segment_update_and_delete() {
        let (pool, path) = test_pool("segments").await;
        let segments = vec![
            NewSegment {
                channel: "you".into(),
                text: "hello".into(),
                start_ms: None,
                end_ms: None,
            },
            NewSegment {
                channel: "others".into(),
                text: "hi there".into(),
                start_ms: None,
                end_ms: None,
            },
        ];
        let id = save_meeting_impl(&pool, "Test", 60, "notes", &segments)
            .await
            .expect("save");

        let detail = get_meeting_impl(&pool, id).await.expect("get");
        assert_eq!(detail.transcript.len(), 2);
        let seg_id = detail.transcript[0].id;

        update_segment_text_impl(&pool, seg_id, "hello world")
            .await
            .expect("update");
        let detail = get_meeting_impl(&pool, id).await.expect("get after update");
        assert_eq!(detail.transcript[0].text, "hello world");

        delete_segment_impl(&pool, seg_id).await.expect("delete");
        let detail = get_meeting_impl(&pool, id).await.expect("get after delete");
        assert_eq!(detail.transcript.len(), 1);

        drop(pool);
        let _ = std::fs::remove_file(path);
    }
}
