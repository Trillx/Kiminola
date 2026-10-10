//! Tauri IPC adapters for the persistence modules. Each command only obtains
//! the pool and translates IPC arguments; behaviour lives in the modules.

use tauri::State;

use super::library::{self, LibraryLocation, LibraryNode, SpaceOut};
use super::meeting_store::{
    self, MeetingDetail, MeetingSummary, MeetingToSave, NewSegment, NoteDraftDetail,
    NoteDraftSummary,
};
use super::templates::{self, Template};
use super::{search, settings, DbState};
use crate::db_safety::DatabaseStatus;

#[tauri::command]
pub async fn database_status(state: State<'_, DbState>) -> Result<DatabaseStatus, String> {
    Ok(state.database.status().await)
}

#[tauri::command]
pub async fn retry_database(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
) -> Result<DatabaseStatus, String> {
    state.database.retry().await?;
    let status = state.database.status().await;
    if status.ready {
        crate::app_lifecycle::restart_after_recovery(&app, &state.database).await?;
    }
    Ok(status)
}

#[tauri::command]
pub async fn restore_database_backup(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    backup: String,
) -> Result<(), String> {
    state.database.restore(&backup).await?;
    crate::app_lifecycle::restart_after_recovery(&app, &state.database).await
}

#[tauri::command]
pub async fn is_onboarding_complete(state: State<'_, DbState>) -> Result<bool, String> {
    settings::onboarding_complete(&state.pool().await?).await
}

#[tauri::command]
pub async fn set_onboarding_complete(
    state: State<'_, DbState>,
    complete: bool,
) -> Result<(), String> {
    settings::set_onboarding_complete(&state.pool().await?, complete).await
}

#[tauri::command]
pub async fn save_meeting(
    state: State<'_, DbState>,
    title: String,
    duration_seconds: i64,
    notepad: String,
    segments: Vec<NewSegment>,
    note_draft_id: Option<i64>,
    location: Option<LibraryLocation>,
) -> Result<i64, String> {
    let meeting = MeetingToSave {
        title: &title,
        duration_seconds,
        notepad: &notepad,
        segments: &segments,
        note_draft_id,
        location,
    };
    meeting_store::save_meeting(&state.pool().await?, meeting).await
}

#[tauri::command]
pub async fn list_meetings(state: State<'_, DbState>) -> Result<Vec<MeetingSummary>, String> {
    meeting_store::list_meetings(&state.pool().await?).await
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, DbState>, id: i64) -> Result<MeetingDetail, String> {
    meeting_store::get_meeting(&state.pool().await?, id).await
}

#[tauri::command]
pub async fn rename_meeting(
    state: State<'_, DbState>,
    meeting_id: i64,
    title: String,
) -> Result<(), String> {
    meeting_store::rename_meeting(&state.pool().await?, meeting_id, &title).await
}

#[tauri::command]
pub async fn update_notes(
    state: State<'_, DbState>,
    meeting_id: i64,
    raw_markdown: String,
) -> Result<(), String> {
    meeting_store::update_notes(&state.pool().await?, meeting_id, &raw_markdown).await
}

#[tauri::command]
pub async fn update_segment_text(
    state: State<'_, DbState>,
    segment_id: i64,
    text: String,
) -> Result<(), String> {
    meeting_store::update_segment_text(&state.pool().await?, segment_id, &text).await
}

#[tauri::command]
pub async fn delete_segment(state: State<'_, DbState>, segment_id: i64) -> Result<(), String> {
    meeting_store::delete_segment(&state.pool().await?, segment_id).await
}

#[tauri::command]
pub async fn create_note_draft(
    state: State<'_, DbState>,
    location: Option<LibraryLocation>,
) -> Result<i64, String> {
    meeting_store::create_note_draft(&state.pool().await?, location.as_ref()).await
}

#[tauri::command]
pub async fn list_note_drafts(state: State<'_, DbState>) -> Result<Vec<NoteDraftSummary>, String> {
    meeting_store::list_note_drafts(&state.pool().await?).await
}

#[tauri::command]
pub async fn get_note_draft(state: State<'_, DbState>, id: i64) -> Result<NoteDraftDetail, String> {
    meeting_store::get_note_draft(&state.pool().await?, id).await
}

#[tauri::command]
pub async fn update_note_draft(
    state: State<'_, DbState>,
    id: i64,
    raw_markdown: String,
) -> Result<(), String> {
    meeting_store::update_note_draft(&state.pool().await?, id, &raw_markdown).await
}

#[tauri::command]
pub async fn update_note_draft_recovery(
    state: State<'_, DbState>,
    id: i64,
    raw_markdown: String,
    duration_seconds: i64,
    transcript: Vec<NewSegment>,
    location: Option<LibraryLocation>,
) -> Result<(), String> {
    meeting_store::update_note_draft_recovery(
        &state.pool().await?,
        id,
        &raw_markdown,
        duration_seconds,
        &transcript,
        location.as_ref(),
    )
    .await
}

#[tauri::command]
pub async fn delete_note_draft(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    meeting_store::delete_note_draft(&state.pool().await?, id).await
}

#[tauri::command]
pub async fn list_spaces(state: State<'_, DbState>) -> Result<Vec<SpaceOut>, String> {
    library::list_spaces(&state.pool().await?).await
}

#[tauri::command]
pub async fn list_library_tree(state: State<'_, DbState>) -> Result<Vec<LibraryNode>, String> {
    library::list_tree(&state.pool().await?).await
}

#[tauri::command]
pub async fn create_space(
    state: State<'_, DbState>,
    name: String,
    parent_space_id: Option<i64>,
) -> Result<i64, String> {
    library::create_space(&state.pool().await?, &name, parent_space_id).await
}

#[tauri::command]
pub async fn rename_space(
    state: State<'_, DbState>,
    space_id: i64,
    name: String,
) -> Result<(), String> {
    library::rename_space(&state.pool().await?, space_id, &name).await
}

#[tauri::command]
pub async fn move_library_node(
    state: State<'_, DbState>,
    node: LibraryLocation,
    destination: Option<LibraryLocation>,
) -> Result<(), String> {
    library::move_node(&state.pool().await?, node, destination).await
}

#[tauri::command]
pub async fn list_templates(state: State<'_, DbState>) -> Result<Vec<Template>, String> {
    templates::list(&state.pool().await?).await
}

#[tauri::command]
pub async fn create_template(
    state: State<'_, DbState>,
    name: String,
    prompt: String,
) -> Result<Template, String> {
    templates::create(&state.pool().await?, &name, &prompt).await
}

#[tauri::command]
pub async fn update_template(
    state: State<'_, DbState>,
    id: i64,
    name: String,
    prompt: String,
) -> Result<(), String> {
    templates::update(&state.pool().await?, id, &name, &prompt).await
}

#[tauri::command]
pub async fn delete_template(state: State<'_, DbState>, id: i64) -> Result<(), String> {
    templates::delete(&state.pool().await?, id).await
}

#[tauri::command]
pub async fn search_meetings(
    state: State<'_, DbState>,
    query: String,
) -> Result<Vec<MeetingSummary>, String> {
    search::search_meetings(&state.pool().await?, &query).await
}
