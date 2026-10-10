//! SQLite persistence (SPEC.md §6). Single bundled database under
//! `%LOCALAPPDATA%\Kiminola\data\kiminola.db`, migrated on first use by
//! [`crate::db_safety::Database`].
//!
//! Modules:
//! - [`meeting_store`]: Meetings, transcripts, notes, Note drafts and the
//!   retryable no-duplicate save.
//! - [`library`]: Spaces, the Meeting hierarchy and Library locations.
//! - [`settings`]: the key-value `settings` table.
//! - [`templates`]: Note enhancement templates.
//! - [`search`]: full-text Meeting search.
//! - [`commands`]: thin Tauri IPC adapters over the modules above.

pub(crate) mod commands;
pub(crate) mod library;
pub(crate) mod meeting_store;
pub(crate) mod search;
pub(crate) mod settings;
pub(crate) mod templates;

use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;
use tauri::Manager;

use crate::db_safety::Database;

/// Shared startup and recovery boundary. Failed initialization stays blocked
/// until the user retries or restores through the recovery screen.
pub struct DbState {
    pub(crate) database: Arc<Database>,
}

impl DbState {
    /// The ready pool, or the startup, recovery or update-suspension error the
    /// UI should surface instead.
    pub(crate) async fn pool(&self) -> Result<SqlitePool, String> {
        self.database.pool().await
    }
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

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Install DB state and warm the pool at launch.
pub fn setup(app: &mut tauri::App) {
    let state = DbState {
        database: Arc::new(Database::new(db_path())),
    };
    let database = Arc::clone(&state.database);
    app.manage(state);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = database.pool().await {
            eprintln!("[db] warm-up failed: {e}");
        }
    });
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use sqlx::SqlitePool;

    /// A fully migrated database in a temp file, opened the same way the app
    /// opens its real database.
    pub(crate) struct TestDb {
        pub(crate) pool: SqlitePool,
        path: PathBuf,
    }

    impl TestDb {
        pub(crate) async fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "kiminola-dbtest-{}-{}-{name}.db",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&path);
            let pool = crate::db_safety::init_pool(&path)
                .await
                .expect("init test pool");
            Self { pool, path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            // Best effort: SQLite may still hold the file until the pool's
            // connections finish closing.
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
