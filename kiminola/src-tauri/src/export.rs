//! Meeting export (SPEC.md §6, §8): notes as Markdown with YAML frontmatter
//! (title, date, space, duration), transcript as plain text. Clipboard exports
//! return the content; file exports write to
//! `%USERPROFILE%\Documents\Kimi Nola\Exports` (fallback: `exports/` next to
//! the executable) and return the written path.

use std::path::PathBuf;

use tauri::State;

use crate::db::{ensure_pool, DbState, MeetingDetail};

fn yaml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Lowercase dash-separated slug safe for filenames; "meeting" if nothing usable.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "meeting".to_string()
    } else {
        slug.to_string()
    }
}

/// `created_at` is stored as `YYYY-MM-DDTHH:MM:SS.sssZ`; take the date part.
fn date_prefix(created_at: &str) -> &str {
    created_at.get(..10).unwrap_or(created_at)
}

/// Mirrors the frontend's `formatMeta`: at least one minute.
fn duration_minutes(duration_seconds: i64) -> i64 {
    ((duration_seconds as f64) / 60.0).round().max(1.0) as i64
}

/// Notes document: YAML frontmatter + enhanced notes when present, else the
/// raw notepad. Raw notes are never modified, only read.
pub fn notes_markdown(meeting: &MeetingDetail) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("title: \"{}\"\n", yaml_escape(&meeting.title)));
    out.push_str(&format!("date: {}\n", date_prefix(&meeting.created_at)));
    if let Some(space) = meeting
        .location_path
        .as_ref()
        .or(meeting.space_name.as_ref())
    {
        out.push_str(&format!("space: \"{}\"\n", yaml_escape(space)));
    }
    out.push_str(&format!(
        "duration_minutes: {}\n",
        duration_minutes(meeting.duration_seconds)
    ));
    out.push_str("---\n\n");
    let body = meeting
        .enhanced_markdown
        .as_deref()
        .filter(|md| !md.trim().is_empty())
        .unwrap_or(&meeting.notepad);
    out.push_str(body.trim());
    out.push('\n');
    out
}

/// Plain-text transcript, one segment per line, labeled like the transcript
/// tab ("You:" / "Others:").
pub fn transcript_text(meeting: &MeetingDetail) -> String {
    meeting
        .transcript
        .iter()
        .map(|s| {
            format!(
                "{}: {}",
                if s.channel == "you" { "You" } else { "Others" },
                s.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn exports_dir() -> Result<PathBuf, String> {
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(profile)
            .join("Documents")
            .join("Kimi Nola")
            .join("Exports"));
    }
    let exe = std::env::current_exe().map_err(|e| format!("no current exe path: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "exe has no parent directory".to_string())?;
    Ok(dir.join("exports"))
}

fn write_export(filename: &str, content: &str) -> Result<String, String> {
    let dir = exports_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create exports dir: {e}"))?;
    let path = dir.join(filename);
    std::fs::write(&path, content).map_err(|e| format!("failed to write export: {e}"))?;
    Ok(path.display().to_string())
}

fn export_filename(meeting: &MeetingDetail, suffix: &str, ext: &str) -> String {
    format!(
        "{}-{}{}.{}",
        date_prefix(&meeting.created_at),
        slugify(&meeting.title),
        suffix,
        ext
    )
}

#[tauri::command]
pub async fn export_notes_markdown(
    state: State<'_, DbState>,
    meeting_id: i64,
) -> Result<String, String> {
    let pool = ensure_pool(&state.pool).await?;
    let meeting = crate::db::get_meeting_impl(&pool, meeting_id).await?;
    Ok(notes_markdown(&meeting))
}

#[tauri::command]
pub async fn export_transcript_text(
    state: State<'_, DbState>,
    meeting_id: i64,
) -> Result<String, String> {
    let pool = ensure_pool(&state.pool).await?;
    let meeting = crate::db::get_meeting_impl(&pool, meeting_id).await?;
    Ok(transcript_text(&meeting))
}

#[tauri::command]
pub async fn save_notes_export(
    state: State<'_, DbState>,
    meeting_id: i64,
) -> Result<String, String> {
    let pool = ensure_pool(&state.pool).await?;
    let meeting = crate::db::get_meeting_impl(&pool, meeting_id).await?;
    let filename = export_filename(&meeting, "", "md");
    write_export(&filename, &notes_markdown(&meeting))
}

#[tauri::command]
pub async fn save_transcript_export(
    state: State<'_, DbState>,
    meeting_id: i64,
) -> Result<String, String> {
    let pool = ensure_pool(&state.pool).await?;
    let meeting = crate::db::get_meeting_impl(&pool, meeting_id).await?;
    let filename = export_filename(&meeting, "-transcript", "txt");
    write_export(&filename, &transcript_text(&meeting))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SegmentOut;

    fn meeting() -> MeetingDetail {
        MeetingDetail {
            id: 1,
            title: "Weekly Sync: Q3/Q4?".into(),
            created_at: "2026-08-16T14:32:00.000Z".into(),
            duration_seconds: 2730,
            space_name: Some("Engineering".into()),
            location_path: None,
            parent_meeting_id: None,
            notepad: "raw notes".into(),
            enhanced_markdown: Some("## Summary\nEnhanced.".into()),
            transcript: vec![
                SegmentOut {
                    id: 10,
                    channel: "you".into(),
                    text: "hi".into(),
                    start_ms: Some(0),
                    end_ms: Some(500),
                },
                SegmentOut {
                    id: 11,
                    channel: "others".into(),
                    text: "hello".into(),
                    start_ms: Some(500),
                    end_ms: Some(1_000),
                },
            ],
        }
    }

    #[test]
    fn slugify_strips_punctuation_and_dashes() {
        assert_eq!(slugify("Weekly Sync: Q3/Q4?"), "weekly-sync-q3-q4");
        assert_eq!(slugify("   "), "meeting");
        assert_eq!(slugify("standup"), "standup");
    }

    #[test]
    fn notes_markdown_frontmatter_and_prefers_enhanced() {
        let md = notes_markdown(&meeting());
        assert!(
            md.starts_with(
                "---\ntitle: \"Weekly Sync: Q3/Q4?\"\ndate: 2026-08-16\nspace: \"Engineering\"\nduration_minutes: 46\n---\n\n"
            ),
            "unexpected frontmatter: {md}"
        );
        assert!(md.ends_with("## Summary\nEnhanced.\n"));
    }

    #[test]
    fn notes_markdown_uses_the_computed_library_location_path() {
        let mut m = meeting();
        m.location_path = Some("Work / Planning".into());
        let md = notes_markdown(&m);
        assert!(md.contains("space: \"Work / Planning\""));
    }

    #[test]
    fn notes_markdown_falls_back_to_raw_notepad() {
        let mut m = meeting();
        m.enhanced_markdown = None;
        assert!(notes_markdown(&m).ends_with("raw notes\n"));
        m.enhanced_markdown = Some("   ".into());
        assert!(notes_markdown(&m).ends_with("raw notes\n"));
    }

    #[test]
    fn transcript_text_labels_channels() {
        assert_eq!(transcript_text(&meeting()), "You: hi\nOthers: hello");
    }
}
