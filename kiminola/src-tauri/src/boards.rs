use std::collections::HashMap;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

use crate::db::{ensure_pool, DbState};

const DEFAULT_BOARD_NAME: &str = "To-Do's";
const DEFAULT_COLUMN_NAMES: [&str; 5] = ["Backlog", "To Do", "Working", "Update", "Done"];

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardCard {
    pub id: i64,
    pub title: String,
    pub position: i64,
    pub meeting_id: Option<i64>,
    pub meeting_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardColumn {
    pub id: i64,
    pub name: String,
    pub position: i64,
    pub cards: Vec<BoardCard>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub columns: Vec<BoardColumn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardsSnapshot {
    pub boards: Vec<Board>,
    pub created_default: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct BoardRow {
    id: i64,
    name: String,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ColumnRow {
    id: i64,
    board_id: i64,
    name: String,
    position: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct CardRow {
    id: i64,
    column_id: i64,
    title: String,
    position: i64,
    meeting_id: Option<i64>,
    meeting_title: Option<String>,
}

async fn ensure_default_board(pool: &SqlitePool) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let board_count: i64 = sqlx::query_scalar("SELECT count(*) FROM boards")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if board_count > 0 {
        tx.commit().await.map_err(|e| e.to_string())?;
        return Ok(false);
    }

    let board_id: i64 =
        sqlx::query_scalar("INSERT INTO boards (name, created_at) VALUES (?, ?) RETURNING id")
            .bind(DEFAULT_BOARD_NAME)
            .bind(now_iso())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

    for (position, name) in DEFAULT_COLUMN_NAMES.iter().enumerate() {
        sqlx::query("INSERT INTO board_columns (board_id, name, position) VALUES (?, ?, ?)")
            .bind(board_id)
            .bind(name)
            .bind(position as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(true)
}

pub(crate) async fn list_boards_impl(pool: &SqlitePool) -> Result<BoardsSnapshot, String> {
    let created_default = ensure_default_board(pool).await?;
    let board_rows = sqlx::query_as::<_, BoardRow>(
        "SELECT id, name, created_at FROM boards ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut boards: Vec<Board> = board_rows
        .into_iter()
        .map(|row| Board {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            columns: Vec::new(),
        })
        .collect();
    let board_indices: HashMap<i64, usize> = boards
        .iter()
        .enumerate()
        .map(|(index, board)| (board.id, index))
        .collect();

    let column_rows = sqlx::query_as::<_, ColumnRow>(
        "SELECT id, board_id, name, position
         FROM board_columns
         ORDER BY board_id ASC, position ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut column_indices = HashMap::new();
    for row in column_rows {
        let Some(&board_index) = board_indices.get(&row.board_id) else {
            continue;
        };
        let column_index = boards[board_index].columns.len();
        boards[board_index].columns.push(BoardColumn {
            id: row.id,
            name: row.name,
            position: row.position,
            cards: Vec::new(),
        });
        column_indices.insert(row.id, (board_index, column_index));
    }

    let card_rows = sqlx::query_as::<_, CardRow>(
        "SELECT c.id, c.column_id, c.title, c.position, c.meeting_id, m.title AS meeting_title
         FROM board_cards c
         LEFT JOIN meetings m ON m.id = c.meeting_id
         ORDER BY c.column_id ASC, c.position ASC, c.id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for row in card_rows {
        let Some(&(board_index, column_index)) = column_indices.get(&row.column_id) else {
            continue;
        };
        boards[board_index].columns[column_index]
            .cards
            .push(BoardCard {
                id: row.id,
                title: row.title,
                position: row.position,
                meeting_id: row.meeting_id,
                meeting_title: row.meeting_title,
            });
    }

    Ok(BoardsSnapshot {
        boards,
        created_default,
    })
}

async fn load_card(pool: &SqlitePool, card_id: i64) -> Result<BoardCard, String> {
    sqlx::query_as::<_, CardRow>(
        "SELECT c.id, c.column_id, c.title, c.position, c.meeting_id, m.title AS meeting_title
         FROM board_cards c
         LEFT JOIN meetings m ON m.id = c.meeting_id
         WHERE c.id = ?",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .map(|row| BoardCard {
        id: row.id,
        title: row.title,
        position: row.position,
        meeting_id: row.meeting_id,
        meeting_title: row.meeting_title,
    })
    .ok_or_else(|| "board card not found".to_string())
}

pub(crate) async fn create_board_impl(pool: &SqlitePool, name: &str) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Board name cannot be empty".to_string());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let board_id: i64 =
        sqlx::query_scalar("INSERT INTO boards (name, created_at) VALUES (?, ?) RETURNING id")
            .bind(name)
            .bind(now_iso())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO board_columns (board_id, name, position) VALUES (?, 'To Do', 0)")
        .bind(board_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(board_id)
}

pub(crate) async fn rename_board_impl(
    pool: &SqlitePool,
    board_id: i64,
    name: &str,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Board name cannot be empty".to_string());
    }
    let changed = sqlx::query("UPDATE boards SET name = ? WHERE id = ?")
        .bind(name)
        .bind(board_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if changed == 0 {
        return Err("Board not found".to_string());
    }
    Ok(())
}

pub(crate) async fn create_column_impl(
    pool: &SqlitePool,
    board_id: i64,
    name: &str,
) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Column name cannot be empty".to_string());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let board_exists: Option<i64> = sqlx::query_scalar("SELECT id FROM boards WHERE id = ?")
        .bind(board_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if board_exists.is_none() {
        return Err("Board not found".to_string());
    }
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM board_columns WHERE board_id = ?",
    )
    .bind(board_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let column_id: i64 = sqlx::query_scalar(
        "INSERT INTO board_columns (board_id, name, position) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(board_id)
    .bind(name)
    .bind(position)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(column_id)
}

pub(crate) async fn rename_column_impl(
    pool: &SqlitePool,
    column_id: i64,
    name: &str,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Column name cannot be empty".to_string());
    }
    let changed = sqlx::query("UPDATE board_columns SET name = ? WHERE id = ?")
        .bind(name)
        .bind(column_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if changed == 0 {
        return Err("Column not found".to_string());
    }
    Ok(())
}

pub(crate) async fn add_card_impl(
    pool: &SqlitePool,
    board_id: i64,
    column_id: i64,
    title: &str,
    meeting_id: Option<i64>,
) -> Result<BoardCard, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Action item cannot be empty".to_string());
    }
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let column_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM board_columns WHERE id = ? AND board_id = ?")
            .bind(column_id)
            .bind(board_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    if column_exists.is_none() {
        return Err("Column does not belong to this board".to_string());
    }
    if let Some(meeting_id) = meeting_id {
        let meeting_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        if meeting_exists.is_none() {
            return Err("Meeting not found".to_string());
        }
    }
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM board_cards WHERE column_id = ?",
    )
    .bind(column_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let card_id: i64 = sqlx::query_scalar(
        "INSERT INTO board_cards (column_id, meeting_id, title, position, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(column_id)
    .bind(meeting_id)
    .bind(title)
    .bind(position)
    .bind(now_iso())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    load_card(pool, card_id).await
}

pub(crate) async fn move_card_impl(
    pool: &SqlitePool,
    card_id: i64,
    column_id: i64,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let target_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM board_columns WHERE id = ?")
            .bind(column_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    if target_exists.is_none() {
        return Err("Column not found".to_string());
    }
    let card_exists: Option<i64> = sqlx::query_scalar("SELECT id FROM board_cards WHERE id = ?")
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if card_exists.is_none() {
        return Err("Board card not found".to_string());
    }
    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM board_cards WHERE column_id = ?",
    )
    .bind(column_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE board_cards SET column_id = ?, position = ? WHERE id = ?")
        .bind(column_id)
        .bind(position)
        .bind(card_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_boards(state: State<'_, DbState>) -> Result<BoardsSnapshot, String> {
    let pool = ensure_pool(&state.pool).await?;
    list_boards_impl(&pool).await
}

#[tauri::command]
pub async fn create_board(state: State<'_, DbState>, name: String) -> Result<i64, String> {
    let pool = ensure_pool(&state.pool).await?;
    create_board_impl(&pool, &name).await
}

#[tauri::command]
pub async fn rename_board(
    state: State<'_, DbState>,
    board_id: i64,
    name: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    rename_board_impl(&pool, board_id, &name).await
}

#[tauri::command]
pub async fn create_board_column(
    state: State<'_, DbState>,
    board_id: i64,
    name: String,
) -> Result<i64, String> {
    let pool = ensure_pool(&state.pool).await?;
    create_column_impl(&pool, board_id, &name).await
}

#[tauri::command]
pub async fn rename_board_column(
    state: State<'_, DbState>,
    column_id: i64,
    name: String,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    rename_column_impl(&pool, column_id, &name).await
}

#[tauri::command]
pub async fn add_board_card(
    state: State<'_, DbState>,
    board_id: i64,
    column_id: i64,
    title: String,
    meeting_id: Option<i64>,
) -> Result<BoardCard, String> {
    let pool = ensure_pool(&state.pool).await?;
    add_card_impl(&pool, board_id, column_id, &title, meeting_id).await
}

#[tauri::command]
pub async fn move_board_card(
    state: State<'_, DbState>,
    card_id: i64,
    column_id: i64,
) -> Result<(), String> {
    let pool = ensure_pool(&state.pool).await?;
    move_card_impl(&pool, card_id, column_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_safety::init_pool;

    #[tokio::test]
    async fn boards_create_customise_and_move_cards() {
        let path = std::env::temp_dir().join(format!(
            "kiminola-boards-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let pool = init_pool(&path).await.expect("init board test pool");

        let first = list_boards_impl(&pool).await.expect("default boards");
        assert!(first.created_default);
        assert_eq!(first.boards[0].name, DEFAULT_BOARD_NAME);
        assert_eq!(
            first.boards[0]
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            DEFAULT_COLUMN_NAMES
        );
        let second = list_boards_impl(&pool).await.expect("existing boards");
        assert!(!second.created_default);

        let board_id = create_board_impl(&pool, "Project follow-ups")
            .await
            .expect("create board");
        let column_id = create_column_impl(&pool, board_id, "Review")
            .await
            .expect("create column");
        rename_column_impl(&pool, column_id, "Ready for review")
            .await
            .expect("rename column");
        let meeting_id: i64 = sqlx::query_scalar(
            "INSERT INTO meetings (title, space_id, created_at, duration_seconds)
             VALUES ('Board meeting', (SELECT id FROM spaces LIMIT 1), ?, 0) RETURNING id",
        )
        .bind(now_iso())
        .fetch_one(&pool)
        .await
        .expect("seed meeting");
        let card = add_card_impl(
            &pool,
            board_id,
            column_id,
            "Send the follow-up",
            Some(meeting_id),
        )
        .await
        .expect("add card");
        move_card_impl(&pool, card.id, second.boards[0].columns[0].id)
            .await
            .expect("move card");

        let snapshot = list_boards_impl(&pool).await.expect("reload boards");
        let default_column = &snapshot.boards[0].columns[0];
        assert_eq!(default_column.cards[0].title, "Send the follow-up");
        assert_eq!(default_column.cards[0].meeting_id, Some(meeting_id));
        assert_eq!(snapshot.boards[1].columns[1].name, "Ready for review");

        pool.close().await;
        std::fs::remove_file(path).expect("remove board test database");
    }
}
