//! Full-text Meeting search over titles, notes and transcripts (SQLite FTS5).
//! Every whitespace-separated term becomes a prefix match (`term*`) and all
//! terms must match. Terms are not quoted, so FTS operators typed by the user
//! are still interpreted (and malformed syntax returns an error).

use sqlx::SqlitePool;

use super::library::{LibraryIndex, MeetingRow};
use super::meeting_store::{summarize, MeetingSummary};

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("{}*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Up to 50 best-ranked Meetings matching every term; blank queries match nothing.
pub(crate) async fn search_meetings(
    pool: &SqlitePool,
    query: &str,
) -> Result<Vec<MeetingSummary>, String> {
    let safe = fts_query(query.trim());
    if safe.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query_as::<_, MeetingRow>(
        "SELECT m.id, m.title, m.created_at, m.duration_seconds, m.space_id, m.parent_meeting_id
         FROM search_index
         JOIN meetings m ON m.id = search_index.rowid
         WHERE search_index MATCH ?
         ORDER BY rank DESC
         LIMIT 50",
    )
    .bind(safe)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let index = LibraryIndex::load(pool).await?;
    rows.iter().map(|row| summarize(row, &index)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::meeting_store::{save_meeting, update_notes, MeetingToSave, NewSegment};
    use crate::db::test_support::TestDb;

    async fn save(pool: &SqlitePool, title: &str, notepad: &str, channel: &str, text: &str) -> i64 {
        let segments = [NewSegment {
            channel: channel.into(),
            text: text.into(),
            start_ms: None,
            end_ms: None,
        }];
        save_meeting(
            pool,
            MeetingToSave {
                title,
                duration_seconds: 60,
                notepad,
                segments: &segments,
                ..MeetingToSave::default()
            },
        )
        .await
        .expect("save")
    }

    async fn ids(pool: &SqlitePool, query: &str) -> Vec<i64> {
        search_meetings(pool, query)
            .await
            .expect("search")
            .into_iter()
            .map(|summary| summary.id)
            .collect()
    }

    #[tokio::test]
    async fn search_indexes_titles_notes_and_transcript() {
        let db = TestDb::new("search").await;
        let pool = &db.pool;
        let id1 = save(
            pool,
            "CLI strategy meeting",
            "discussed CLI and VPS",
            "you",
            "we need a CLI",
        )
        .await;
        let id2 = save(
            pool,
            "Design sync",
            "colors and buttons",
            "others",
            "the navbar needs work",
        )
        .await;

        assert_eq!(ids(pool, "CLI").await, vec![id1], "title");
        assert_eq!(ids(pool, "VPS").await, vec![id1], "notes");
        assert_eq!(ids(pool, "navbar").await, vec![id2], "transcript");
        assert_eq!(ids(pool, "strat").await, vec![id1], "prefix");

        update_notes(pool, id2, "launch checklist").await.unwrap();
        assert_eq!(
            ids(pool, "checklist").await,
            vec![id2],
            "edited notes are indexed"
        );

        let summary = &search_meetings(pool, "navbar").await.unwrap()[0];
        assert_eq!(summary.space_name.as_deref(), Some("Personal"));
    }

    #[tokio::test]
    async fn blank_queries_match_nothing_and_every_term_must_match() {
        let db = TestDb::new("search-terms").await;
        let id = save(&db.pool, "Quarterly plan", "", "you", "budget review").await;
        assert!(ids(&db.pool, "   ").await.is_empty());
        assert_eq!(ids(&db.pool, "quart budg").await, vec![id]);
        assert!(ids(&db.pool, "quarterly missing").await.is_empty());
    }
}
