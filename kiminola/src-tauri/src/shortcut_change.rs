//! Serialized, compensating shortcut changes, independent of Tauri and SQLite.
//!
//! The old registration stays active through validation, candidate registration,
//! and persistence. Only a successfully saved replacement can retire it. There
//! is no atomic transaction spanning the OS and SQLite: compensation failures
//! are returned explicitly, and leftover inactive registrations are retained for
//! cleanup on the next attempt. Callers must let an in-flight change finish even
//! if its IPC waiter disappears (the command runs it in an owned task).

use std::sync::Mutex;
use tokio::sync::Mutex as AsyncMutex;

#[async_trait::async_trait]
pub(super) trait ShortcutBackend: Send + Sync {
    type Shortcut: PartialEq + Send + Sync;

    fn parse(&self, value: &str) -> Result<Self::Shortcut, String>;
    // Like the plugin's single-shortcut methods, errors leave ownership unchanged.
    fn register(&self, shortcut: &Self::Shortcut) -> Result<(), String>;
    fn unregister(&self, shortcut: &Self::Shortcut) -> Result<(), String>;
    async fn load(&self) -> Result<Option<String>, String>;
    async fn save(&self, shortcut: Option<&str>) -> Result<(), String>;
}

pub(super) async fn replace<B: ShortcutBackend>(
    current: &Mutex<Option<String>>,
    changes: &AsyncMutex<Vec<String>>,
    backend: &B,
    requested: Option<String>,
) -> Result<(), String> {
    // This separate async lock covers reads, OS changes, writes and compensation.
    // Never hold the handler's synchronous `current` lock over an await or an OS
    // call: the plugin dispatches to the main thread, which also runs the handler.
    let mut pending_cleanup = changes.lock().await;
    let stored = requested.filter(|value| !value.trim().is_empty());
    let candidate = stored
        .as_deref()
        .map(|value| backend.parse(value))
        .transpose()?;
    let old = current.lock().unwrap().clone();
    let previous = old
        .as_deref()
        .map(|value| backend.parse(value))
        .transpose()?;

    cleanup(backend, &mut pending_cleanup)
        .map_err(|error| format!("shortcut cleanup failed: {error}; active shortcut unchanged"))?;
    let saved = backend.load().await?;
    let registration_changed = candidate != previous;

    if registration_changed {
        if let Some(candidate) = candidate.as_ref() {
            // Register before retiring the old binding. Invalid/unavailable input
            // can never disable the working shortcut, even transiently.
            backend.register(candidate)?;
            pending_cleanup.push(stored.as_ref().unwrap().clone());
        }
    }

    if let Err(error) = backend.save(stored.as_deref()).await {
        return Err(rollback(backend, &mut pending_cleanup, saved.as_deref(), error).await);
    }

    if registration_changed {
        if let Some(previous) = previous.as_ref() {
            if let Err(error) = backend.unregister(previous) {
                return Err(rollback(backend, &mut pending_cleanup, saved.as_deref(), error).await);
            }
        }
    }

    // No await between retiring the old registration and publishing the new
    // event-filter value. Equivalent parsed accelerators never touch the OS.
    *current.lock().unwrap() = stored;
    pending_cleanup.clear();
    Ok(())
}

fn cleanup<B: ShortcutBackend>(backend: &B, pending: &mut Vec<String>) -> Result<(), String> {
    while let Some(value) = pending.last() {
        let shortcut = backend.parse(value)?;
        backend.unregister(&shortcut)?;
        pending.pop();
    }
    Ok(())
}

async fn rollback<B: ShortcutBackend>(
    backend: &B,
    pending: &mut Vec<String>,
    saved: Option<&str>,
    cause: String,
) -> String {
    let mut error = format!("{cause}; active shortcut unchanged");
    if let Err(cleanup_error) = cleanup(backend, pending) {
        error.push_str(&format!(
            "; rollback failed to unregister replacement: {cleanup_error}; \
             replacement remains reserved but inactive; retry to clean it up"
        ));
    }
    if let Err(save_error) = restore_saved(backend, saved).await {
        error.push_str(&format!(
            "; rollback failed to restore saved shortcut: {save_error}; \
             saved and active shortcuts may differ"
        ));
    }
    error
}

async fn restore_saved<B: ShortcutBackend>(backend: &B, saved: Option<&str>) -> Result<(), String> {
    // A failed write normally leaves SQLite untouched. Read first to avoid a
    // redundant write, but also handle a commit whose acknowledgement was lost.
    if let Ok(actual) = backend.load().await {
        if actual.as_deref() == saved {
            return Ok(());
        }
    }
    backend.save(saved).await?;
    if backend.load().await?.as_deref() != saved {
        return Err("saved shortcut did not match the pre-change value".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "shortcut_change_tests.rs"]
mod tests;
