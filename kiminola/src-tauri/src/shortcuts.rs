//! Global shortcut configuration and runtime registration.
//!
//! The configured accelerator is stored in SQLite `settings` under
//! `global_shortcut`. When triggered, the backend emits `shortcut:triggered`;
//! the frontend decides whether to start or stop recording.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};

use crate::db::{settings, DbState};

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
    settings::get(&state.pool().await?, SHORTCUT_KEY).await
}

#[tauri::command]
pub async fn set_global_shortcut(
    shortcut: Option<String>,
    app: AppHandle,
    db: State<'_, DbState>,
    shortcut_state: State<'_, ShortcutState>,
) -> Result<(), String> {
    let pool = db.pool().await?;

    // Unregister the old shortcut, if any.
    if let Some(old) = shortcut_state.current.lock().unwrap().take() {
        unregister_shortcut(&app, &old)?;
    }

    let stored = shortcut.filter(|s| !s.trim().is_empty());

    if let Some(s) = stored.as_ref() {
        register_shortcut(&app, s)?;
        settings::set(&pool, SHORTCUT_KEY, s).await?;
    } else {
        settings::remove(&pool, SHORTCUT_KEY).await?;
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
        let pool = db_state.pool().await?;
        if let Some(s) = settings::get(&pool, SHORTCUT_KEY).await? {
            register_shortcut(&handle, &s)?;
            if let Some(state) = handle.try_state::<ShortcutState>() {
                *state.current.lock().unwrap() = Some(s);
            }
        }
        Ok::<(), String>(())
    })?;

    Ok(())
}
