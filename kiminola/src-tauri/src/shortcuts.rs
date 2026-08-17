//! Global shortcut configuration and runtime registration.
//!
//! The configured accelerator is stored in SQLite `settings` under
//! `global_shortcut`. When triggered, the backend emits `shortcut:triggered`;
//! the frontend decides whether to start or stop recording.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};

use crate::db::{ensure_pool, DbState};

const SHORTCUT_KEY: &str = "global_shortcut";

pub struct ShortcutState {
    pub current: Mutex<Option<String>>,
}

impl ShortcutState {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub async fn get_global_shortcut(state: State<'_, DbState>) -> Result<Option<String>, String> {
    let pool = ensure_pool(&state.pool).await?;
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(SHORTCUT_KEY)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_global_shortcut(
    shortcut: Option<String>,
    app: AppHandle,
    db: State<'_, DbState>,
    shortcut_state: State<'_, ShortcutState>,
) -> Result<(), String> {
    let pool = ensure_pool(&db.pool).await?;

    // Unregister the old shortcut, if any.
    if let Some(old) = shortcut_state.current.lock().unwrap().take() {
        unregister_shortcut(&app, &old)?;
    }

    let stored = shortcut.filter(|s| !s.trim().is_empty());

    if let Some(s) = stored.as_ref() {
        register_shortcut(&app, s)?;
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(SHORTCUT_KEY)
        .bind(s)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(SHORTCUT_KEY)
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
    }

    *shortcut_state.current.lock().unwrap() = stored;
    Ok(())
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use std::str::FromStr;
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
        let shortcut = Shortcut::from_str(shortcut)
            .map_err(|e| format!("invalid shortcut '{shortcut}': {e}"))?;
        app.global_shortcut()
            .register(shortcut)
            .map_err(|e| format!("failed to register shortcut: {e}"))?;
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        let _ = shortcut;
    }
    Ok(())
}

fn unregister_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use std::str::FromStr;
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
        let shortcut = Shortcut::from_str(shortcut)
            .map_err(|e| format!("invalid shortcut '{shortcut}': {e}"))?;
        app.global_shortcut()
            .unregister(shortcut)
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
        let db_state = handle.state::<DbState>();
        let pool = ensure_pool(&db_state.pool).await?;
        let saved: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(SHORTCUT_KEY)
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(s) = saved {
            register_shortcut(&handle, &s)?;
            if let Some(state) = handle.try_state::<ShortcutState>() {
                *state.current.lock().unwrap() = Some(s);
            }
        }
        Ok::<(), String>(())
    })?;

    Ok(())
}
