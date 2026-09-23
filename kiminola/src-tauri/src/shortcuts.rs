//! Global shortcut configuration and runtime registration.
//!
//! The configured accelerator is stored in SQLite `settings` under
//! `global_shortcut`. When triggered, the backend emits `shortcut:triggered`;
//! the frontend decides whether to start or stop recording.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};

use crate::db::{ensure_pool, DbState};

#[path = "shortcut_change.rs"]
mod change;

const SHORTCUT_KEY: &str = "global_shortcut";

pub struct ShortcutState {
    // Coordinate Meeting and Dictation registration transactions. The plugin
    // indexes both by the same accelerator ID.
    pub(crate) coordination: tokio::sync::Mutex<()>,
    pub(crate) dictation_current: Mutex<Option<String>>,
    // Keep this synchronous: lib.rs reads it inside the global shortcut handler.
    pub current: Mutex<Option<String>>,
    // Serialize all updates; retain inactive registrations if rollback fails.
    changes: tokio::sync::Mutex<Vec<String>>,
}

impl ShortcutState {
    pub fn new() -> Self {
        Self {
            coordination: tokio::sync::Mutex::new(()),
            dictation_current: Mutex::new(None),
            current: Mutex::new(None),
            changes: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

#[tauri::command]
pub async fn get_global_shortcut(
    state: State<'_, DbState>,
    shortcut_state: State<'_, ShortcutState>,
) -> Result<Option<String>, String> {
    // Do not expose an intermediate saved value during replacement/rollback.
    let _change = shortcut_state.changes.lock().await;
    let pool = ensure_pool(&state.pool).await?;
    load_saved_shortcut(&pool).await
}

async fn load_saved_shortcut(pool: &sqlx::SqlitePool) -> Result<Option<String>, String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(SHORTCUT_KEY)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_global_shortcut(
    shortcut: Option<String>,
    app: AppHandle,
    db: State<'_, DbState>,
) -> Result<(), String> {
    let pool = ensure_pool(&db.pool).await?;
    // Own the task so dropping the IPC waiter cannot cancel a change between OS
    // registration and its persistence/compensation. The task holds the gate.
    tauri::async_runtime::spawn(async move {
        let state = app.state::<ShortcutState>();
        let _coordination = state.coordination.lock().await;
        if let Some(value) = shortcut.as_deref().filter(|value| !value.trim().is_empty()) {
            let candidate = parse_shortcut(value)?;
            let reserved = state.dictation_current.lock().unwrap().clone();
            if reserved
                .as_deref()
                .map(parse_shortcut)
                .transpose()?
                .as_ref()
                == Some(&candidate)
            {
                return Err(
                    "This shortcut is already used by Dictation. Choose a different shortcut."
                        .into(),
                );
            }
        }
        let backend = NativeShortcutBackend {
            app: &app,
            pool: &pool,
        };
        change::replace(&state.current, &state.changes, &backend, shortcut).await
    })
    .await
    .map_err(|error| format!("shortcut update task failed: {error}"))?
}

struct NativeShortcutBackend<'a> {
    app: &'a AppHandle,
    pool: &'a sqlx::SqlitePool,
}

#[cfg(desktop)]
type ParsedShortcut = tauri_plugin_global_shortcut::Shortcut;
#[cfg(not(desktop))]
type ParsedShortcut = String;

#[async_trait::async_trait]
impl change::ShortcutBackend for NativeShortcutBackend<'_> {
    type Shortcut = ParsedShortcut;

    fn parse(&self, value: &str) -> Result<ParsedShortcut, String> {
        parse_shortcut(value)
    }

    fn register(&self, shortcut: &ParsedShortcut) -> Result<(), String> {
        register_shortcut(self.app, shortcut)
    }

    fn unregister(&self, shortcut: &ParsedShortcut) -> Result<(), String> {
        unregister_shortcut(self.app, shortcut)
    }

    async fn load(&self) -> Result<Option<String>, String> {
        load_saved_shortcut(self.pool).await
    }

    async fn save(&self, shortcut: Option<&str>) -> Result<(), String> {
        if let Some(shortcut) = shortcut {
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )
            .bind(SHORTCUT_KEY)
            .bind(shortcut)
            .execute(self.pool)
            .await
            .map_err(|e| e.to_string())?;
        } else {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(SHORTCUT_KEY)
                .execute(self.pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn parse_shortcut(shortcut: &str) -> Result<ParsedShortcut, String> {
    #[cfg(desktop)]
    {
        shortcut
            .parse::<ParsedShortcut>()
            .map_err(|e| format!("invalid shortcut '{shortcut}': {e}"))
    }
    #[cfg(not(desktop))]
    {
        Ok(shortcut.to_owned())
    }
}

fn register_shortcut(app: &AppHandle, shortcut: &ParsedShortcut) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        app.global_shortcut()
            .register(*shortcut)
            .map_err(|e| format!("failed to register shortcut: {e}"))?;
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        let _ = shortcut;
    }
    Ok(())
}

fn unregister_shortcut(app: &AppHandle, shortcut: &ParsedShortcut) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        app.global_shortcut()
            .unregister(*shortcut)
            .map_err(|e| format!("failed to unregister shortcut: {e}"))?;
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        let _ = shortcut;
    }
    Ok(())
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    tauri::async_runtime::block_on(async move {
        let state = handle.state::<ShortcutState>();
        let _change = state.changes.lock().await;
        let db_state = handle.state::<DbState>();
        let pool = ensure_pool(&db_state.pool).await?;
        if let Some(saved) = load_saved_shortcut(&pool).await? {
            let parsed = parse_shortcut(&saved)?;
            register_shortcut(&handle, &parsed)?;
            *state.current.lock().unwrap() = Some(saved);
        }
        Ok::<(), String>(())
    })?;

    Ok(())
}
