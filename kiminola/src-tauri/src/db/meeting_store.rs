//! Meetings, their transcripts and notes, and Note drafts.
//!
//! Interface:
//! - [`save_meeting`] persists a finished Capture session (Meeting row,
//!   transcript, notes, Library location) in one transaction. It is
//!   **retryable without duplicates** when a Note draft is supplied: the draft
//!   is the idempotency key. Once a save attaches the draft, any later save for
//!   that draft returns the already-saved Meeting id. Saves without a draft are
//!   not idempotent.
//! - Note drafts are editable only while unattached. Their recovery
//!   checkpoint (notepad, transcript, duration, Library location) lets an
//!   interrupted Capture session be saved after a restart.
//! - Reads return Meetings with their resolved Library location path.

use sqlx::SqlitePool;

use super::library::{self, LibraryIndex, LibraryLocation, MeetingRow};
use super::now_iso;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct NewSegment {
    pub channel: String,
    pub text: String,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub struct MeetingSummary {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub duration_seconds: i64,
    pub space_name: Option<String>,
    pub location_path: Option<String>,
    pub parent_meeting_id: Option<i64>,
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
    pub location_path: Option<String>,
    pub parent_meeting_id: Option<i64>,
    pub notepad: String,
    pub enhanced_markdown: Option<String>,
    pub transcript: Vec<SegmentOut>,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct NoteDraftSummary {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, serde::Serialize)]
pub struct NoteDraftDetail {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub raw_markdown: String,
    pub meeting_id: Option<i64>,
    pub recovery_duration_seconds: i64,
    pub recovery_transcript: Vec<NewSegment>,
    pub recovery_location: Option<LibraryLocation>,
}

#[derive(sqlx::FromRow)]
struct NoteDraftRow {
    id: i64,
    title: String,
    created_at: String,
    updated_at: String,
    raw_markdown: String,
    meeting_id: Option<i64>,
    recovery_duration_seconds: i64,
    recovery_transcript_json: String,
    recovery_location_json: Option<String>,
}

/// Everything a finished Capture session contributes to a new Meeting.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MeetingToSave<'a> {
    pub(crate) title: &'a str,
    pub(crate) duration_seconds: i64,
    /// Notepad text. When empty and a Note draft is attached, the draft's
    /// notepad is kept instead.
    pub(crate) notepad: &'a str,
    pub(crate) segments: &'a [NewSegment],
    /// The Note draft this recording started from; also the retry key.
    pub(crate) note_draft_id: Option<i64>,
    /// `None` saves into the default Space.
    pub(crate) location: Option<LibraryLocation>,
}

pub(crate) async fn save_meeting(
    pool: &SqlitePool,
    meeting: MeetingToSave<'_>,
) -> Result<i64, String> {
    if let Some(draft_id) = meeting.note_draft_id {
        let attached_meeting =
            sqlx::query_scalar::<_, Option<i64>>("SELECT meeting_id FROM note_drafts WHERE id = ?")
                .bind(draft_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "note draft not found".to_string())?;
        if let Some(meeting_id) = attached_meeting {
            // The previous save committed but its IPC response may have been
            // lost. Treat the attached draft as the idempotency key so a UI
            // retry returns the existing meeting instead of duplicating it.
            return Ok(meeting_id);
        }
    }

    let (space_id, parent_meeting_id) =
        library::resolve_destination(pool, meeting.location).await?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let meeting_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO meetings (title, space_id, parent_meeting_id, created_at, duration_seconds)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(meeting.title)
    .bind(space_id)
    .bind(parent_meeting_id)
    .bind(now_iso())
    .bind(meeting.duration_seconds)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    for segment in meeting.segments {
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

    let draft_markdown = if let Some(draft_id) = meeting.note_draft_id {
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
    let saved_notepad = if meeting.note_draft_id.is_some() && meeting.notepad.is_empty() {
        &draft_markdown
    } else {
        meeting.notepad
    };

    sqlx::query("INSERT INTO notes (meeting_id, raw_markdown, updated_at) VALUES (?, ?, ?)")
        .bind(meeting_id)
        .bind(saved_notepad)
        .bind(now_iso())
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(draft_id) = meeting.note_draft_id {
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

pub(crate) fn summarize(row: &MeetingRow, index: &LibraryIndex) -> Result<MeetingSummary, String> {
    let placement = index.locate(row.id)?;
    Ok(MeetingSummary {
        id: row.id,
        title: row.title.clone(),
        created_at: row.created_at.clone(),
        duration_seconds: row.duration_seconds,
        space_name: placement.space_name,
        location_path: placement.location_path,
        parent_meeting_id: row.parent_meeting_id,
    })
}

/// Every Meeting, newest first.
pub(crate) async fn list_meetings(pool: &SqlitePool) -> Result<Vec<MeetingSummary>, String> {
    let index = LibraryIndex::load(pool).await?;
    index.meetings().map(|row| summarize(row, &index)).collect()
}

pub(crate) async fn get_meeting(pool: &SqlitePool, id: i64) -> Result<MeetingDetail, String> {
    let index = LibraryIndex::load(pool).await?;
    let row = index
        .meeting(id)
        .ok_or_else(|| "meeting not found".to_string())?;
    let placement = index.locate(id)?;

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
        id: row.id,
        title: row.title.clone(),
        created_at: row.created_at.clone(),
        duration_seconds: row.duration_seconds,
        space_name: placement.space_name,
        location_path: placement.location_path,
        parent_meeting_id: row.parent_meeting_id,
        notepad,
        enhanced_markdown,
        transcript,
    })
}

pub(crate) async fn rename_meeting(
    pool: &SqlitePool,
    meeting_id: i64,
    title: &str,
) -> Result<(), String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Meeting title cannot be empty".to_string());
    }
    let rows = sqlx::query("UPDATE meetings SET title = ? WHERE id = ?")
        .bind(trimmed)
        .bind(meeting_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("Meeting not found".to_string());
    }
    Ok(())
}

/// Replace the user's Notepad text; enhanced notes are left untouched.
pub(crate) async fn update_notes(
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

/// Replace the Note enhancement output; the Notepad is left untouched.
pub(crate) async fn update_enhanced_notes(
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

pub(crate) async fn update_segment_text(
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

pub(crate) async fn delete_segment(pool: &SqlitePool, segment_id: i64) -> Result<(), String> {
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

/* ---------- Note drafts ---------- */

fn note_draft_title() -> String {
    format!(
        "Note draft · {}",
        chrono::Local::now().format("%b %-d, %Y, %-I:%M %p")
    )
}

pub(super) fn encode_location(
    location: Option<&LibraryLocation>,
) -> Result<Option<String>, String> {
    location
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| format!("encode recovery location: {e}"))
}

fn decode_location(value: Option<&str>) -> Result<Option<LibraryLocation>, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| format!("decode recovery location: {e}"))
}

/// Create an empty, unattached Note draft. `location` is the Library location
/// a recording started from this draft should be saved into.
pub(crate) async fn create_note_draft(
    pool: &SqlitePool,
    location: Option<&LibraryLocation>,
) -> Result<i64, String> {
    let now = now_iso();
    let location_json = encode_location(location)?;
    sqlx::query(
        "INSERT INTO note_drafts
            (title, created_at, updated_at, raw_markdown, recovery_location_json)
         VALUES (?, ?, ?, '', ?)",
    )
    .bind(note_draft_title())
    .bind(&now)
    .bind(&now)
    .bind(location_json)
    .execute(pool)
    .await
    .map_err(|e| format!("failed to create note draft: {e}"))?;
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())
}

/// Unattached Note drafts, most recently edited first.
pub(crate) async fn list_note_drafts(pool: &SqlitePool) -> Result<Vec<NoteDraftSummary>, String> {
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

pub(crate) async fn get_note_draft(pool: &SqlitePool, id: i64) -> Result<NoteDraftDetail, String> {
    let row = sqlx::query_as::<_, NoteDraftRow>(
        "SELECT id, title, created_at, updated_at, raw_markdown, meeting_id,
                recovery_duration_seconds, recovery_transcript_json,
                recovery_location_json
         FROM note_drafts
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "note draft not found".to_string())?;
    let recovery_transcript = serde_json::from_str(&row.recovery_transcript_json)
        .map_err(|e| format!("invalid recovery transcript: {e}"))?;
    let recovery_location = decode_location(row.recovery_location_json.as_deref())?;
    Ok(NoteDraftDetail {
        id: row.id,
        title: row.title,
        created_at: row.created_at,
        updated_at: row.updated_at,
        raw_markdown: row.raw_markdown,
        meeting_id: row.meeting_id,
        recovery_duration_seconds: row.recovery_duration_seconds,
        recovery_transcript,
        recovery_location,
    })
}

pub(crate) async fn update_note_draft(
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

/// Checkpoint an in-progress Capture session onto its unattached Note draft.
pub(crate) async fn update_note_draft_recovery(
    pool: &SqlitePool,
    id: i64,
    raw_markdown: &str,
    duration_seconds: i64,
    transcript: &[NewSegment],
    location: Option<&LibraryLocation>,
) -> Result<(), String> {
    let transcript_json = serde_json::to_string(transcript)
        .map_err(|e| format!("encode recovery transcript: {e}"))?;
    let location_json = encode_location(location)?;
    let rows = sqlx::query(
        "UPDATE note_drafts SET raw_markdown = ?, recovery_duration_seconds = ?,
             recovery_transcript_json = ?, recovery_location_json = ?, updated_at = ?
         WHERE id = ? AND meeting_id IS NULL",
    )
    .bind(raw_markdown)
    .bind(duration_seconds.max(0))
    .bind(transcript_json)
    .bind(location_json)
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

pub(crate) async fn delete_note_draft(pool: &SqlitePool, id: i64) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::TestDb;

    fn segment(
        channel: &str,
        text: &str,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> NewSegment {
        NewSegment {
            channel: channel.into(),
            text: text.into(),
            start_ms,
            end_ms,
        }
    }

    #[tokio::test]
    async fn save_list_get_roundtrip() {
        let db = TestDb::new("roundtrip").await;
        let pool = &db.pool;

        let segments = vec![
            segment("others", "hello there", Some(100), Some(1_200)),
            segment("you", "hi", Some(1_250), Some(1_600)),
        ];
        let id = save_meeting(
            pool,
            MeetingToSave {
                title: "Test meeting",
                duration_seconds: 95,
                notepad: "my notes",
                segments: &segments,
                ..MeetingToSave::default()
            },
        )
        .await
        .expect("save");

        let list = list_meetings(pool).await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Test meeting");
        assert_eq!(list[0].space_name.as_deref(), Some("Personal"));

        let detail = get_meeting(pool, id).await.expect("get");
        assert_eq!(detail.duration_seconds, 95);
        assert_eq!(detail.notepad, "my notes");
        assert_eq!(detail.transcript.len(), 2);
        assert_eq!(detail.transcript[0].channel, "others");
        assert_eq!(detail.transcript[0].start_ms, Some(100));
        assert_eq!(detail.transcript[0].end_ms, Some(1_200));

        update_notes(pool, id, "edited notes")
            .await
            .expect("update notes");
        update_enhanced_notes(pool, id, "## Summary").await.unwrap();
        let detail = get_meeting(pool, id).await.expect("get again");
        assert_eq!(detail.notepad, "edited notes");
        assert_eq!(detail.enhanced_markdown.as_deref(), Some("## Summary"));

        // Saves without a Note draft always create a new Meeting.
        let id2 = save_meeting(
            pool,
            MeetingToSave {
                title: "Second",
                duration_seconds: 10,
                ..MeetingToSave::default()
            },
        )
        .await
        .expect("save 2");
        assert_ne!(id, id2);
        let list = list_meetings(pool).await.expect("list 2");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2, "newest Meeting first");

        let spaces = library::list_spaces(pool).await.expect("spaces");
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].meetings.len(), 2);

        rename_meeting(pool, id, "  Renamed  ").await.unwrap();
        assert_eq!(get_meeting(pool, id).await.unwrap().title, "Renamed");
        assert!(rename_meeting(pool, id, " ").await.is_err());
        assert!(rename_meeting(pool, 999, "Missing").await.is_err());
    }

    #[tokio::test]
    async fn missing_meeting_errors() {
        let db = TestDb::new("missing").await;
        let err = get_meeting(&db.pool, 999).await.unwrap_err();
        assert_eq!(err, "meeting not found");
    }

    #[tokio::test]
    async fn note_draft_roundtrip_and_meeting_attach() {
        let db = TestDb::new("note-draft").await;
        let pool = &db.pool;
        let draft_id = create_note_draft(pool, None).await.expect("create draft");

        update_note_draft(pool, draft_id, "follow up with the team")
            .await
            .expect("update draft");
        let recovery_transcript =
            vec![segment("others", "recovered words", Some(500), Some(1_500))];
        update_note_draft_recovery(
            pool,
            draft_id,
            "follow up with the team",
            37,
            &recovery_transcript,
            None,
        )
        .await
        .expect("update recovery snapshot");
        let draft = get_note_draft(pool, draft_id).await.expect("get draft");
        assert_eq!(draft.raw_markdown, "follow up with the team");
        assert_eq!(draft.recovery_duration_seconds, 37);
        assert_eq!(draft.recovery_transcript, recovery_transcript);
        assert!(draft.meeting_id.is_none());
        assert_eq!(list_note_drafts(pool).await.unwrap().len(), 1);

        let save = MeetingToSave {
            title: "Follow-up meeting",
            duration_seconds: 60,
            note_draft_id: Some(draft_id),
            ..MeetingToSave::default()
        };
        let meeting_id = save_meeting(pool, save)
            .await
            .expect("save meeting with draft");
        // A retry after a lost IPC response must not duplicate the Meeting.
        let retried_meeting_id = save_meeting(pool, save)
            .await
            .expect("retry attached draft save");
        assert_eq!(retried_meeting_id, meeting_id);

        let meeting = get_meeting(pool, meeting_id).await.expect("get meeting");
        assert_eq!(meeting.notepad, "follow up with the team");
        assert!(list_note_drafts(pool).await.unwrap().is_empty());
        assert_eq!(list_meetings(pool).await.unwrap().len(), 1);
        assert_eq!(
            get_note_draft(pool, draft_id).await.unwrap().meeting_id,
            Some(meeting_id)
        );

        // An attached draft is frozen.
        assert!(update_note_draft(pool, draft_id, "late edit")
            .await
            .is_err());
        assert!(
            update_note_draft_recovery(pool, draft_id, "late", 1, &[], None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn explicit_notepad_wins_over_the_draft_notepad() {
        let db = TestDb::new("note-draft-notepad").await;
        let draft_id = create_note_draft(&db.pool, None).await.unwrap();
        update_note_draft(&db.pool, draft_id, "draft text")
            .await
            .unwrap();
        let id = save_meeting(
            &db.pool,
            MeetingToSave {
                title: "Meeting",
                notepad: "typed during capture",
                note_draft_id: Some(draft_id),
                ..MeetingToSave::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            get_meeting(&db.pool, id).await.unwrap().notepad,
            "typed during capture"
        );
    }

    #[tokio::test]
    async fn saving_with_an_unknown_or_deleted_draft_fails() {
        let db = TestDb::new("note-draft-missing").await;
        let draft_id = create_note_draft(&db.pool, None).await.unwrap();
        delete_note_draft(&db.pool, draft_id).await.unwrap();
        assert!(delete_note_draft(&db.pool, draft_id).await.is_err());
        let error = save_meeting(
            &db.pool,
            MeetingToSave {
                title: "Meeting",
                note_draft_id: Some(draft_id),
                ..MeetingToSave::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error, "note draft not found");
        assert!(list_meetings(&db.pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn recording_recovery_preserves_library_location() {
        let db = TestDb::new("note-draft-location").await;
        let space_id = library::create_space(&db.pool, "Recovery", None)
            .await
            .expect("create recovery Space");
        let location = LibraryLocation::Space { id: space_id };
        let draft_id = create_note_draft(&db.pool, Some(&location))
            .await
            .expect("create recovery draft");
        assert_eq!(
            get_note_draft(&db.pool, draft_id)
                .await
                .unwrap()
                .recovery_location,
            Some(location)
        );

        update_note_draft_recovery(&db.pool, draft_id, "notes", 12, &[], Some(&location))
            .await
            .expect("save recovery location");
        let draft = get_note_draft(&db.pool, draft_id)
            .await
            .expect("get recovery draft");
        assert_eq!(draft.recovery_location, Some(location));
    }

    #[tokio::test]
    async fn segment_update_and_delete() {
        let db = TestDb::new("segments").await;
        let segments = vec![
            segment("you", "hello", None, None),
            segment("others", "hi there", None, None),
        ];
        let id = save_meeting(
            &db.pool,
            MeetingToSave {
                title: "Test",
                duration_seconds: 60,
                notepad: "notes",
                segments: &segments,
                ..MeetingToSave::default()
            },
        )
        .await
        .expect("save");

        let detail = get_meeting(&db.pool, id).await.expect("get");
        assert_eq!(detail.transcript.len(), 2);
        let seg_id = detail.transcript[0].id;

        update_segment_text(&db.pool, seg_id, "hello world")
            .await
            .expect("update");
        let detail = get_meeting(&db.pool, id).await.expect("get after update");
        assert_eq!(detail.transcript[0].text, "hello world");

        delete_segment(&db.pool, seg_id).await.expect("delete");
        let detail = get_meeting(&db.pool, id).await.expect("get after delete");
        assert_eq!(detail.transcript.len(), 1);
        assert!(delete_segment(&db.pool, seg_id).await.is_err());
        assert!(update_segment_text(&db.pool, seg_id, "gone").await.is_err());
    }
}
