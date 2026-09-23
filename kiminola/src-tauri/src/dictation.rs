//! Backend-owned dictation lifecycle. Native capture, provider and delivery are
//! injected at the actual OS/network boundaries; IPC delegates to these commands.
use crate::capture_gate::{CaptureGate, CaptureLease, CaptureOwner};
use crate::db_safety::Database;
use crate::dictation_history::{self as history, HistoryEntry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{watch, Mutex as AsyncMutex};
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    Hold,
    Toggle,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cleanup {
    Raw,
    Provider,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationSettings {
    pub enabled: bool,
    pub activation: Activation,
    pub shortcut: String,
    pub side: Side,
    pub cleanup: Cleanup,
    pub clipboard_consent: bool,
    pub history_enabled: bool,
    pub microphone_id: Option<String>,
}
impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            activation: Activation::Hold,
            shortcut: "Ctrl+Shift+Space".into(),
            side: Side::Right,
            cleanup: Cleanup::Raw,
            clipboard_consent: false,
            history_enabled: false,
            microphone_id: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsInput {
    #[serde(flatten)]
    pub settings: DictationSettings,
    #[serde(default)]
    pub consent_provider: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSettings {
    pub settings: DictationSettings,
    pub provider_identity: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Disabled,
    Idle,
    Starting,
    Listening,
    Processing,
    Review,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Starting | Self::Listening | Self::Processing)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitIntent {
    Quit,
    Disable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    None,
    Verified,
    Uncertain,
}
#[derive(Debug, Clone, Serialize)]
pub struct DictationSnapshot {
    pub settings: DictationSettings,
    pub provider_authorized: bool,
    pub phase: Phase,
    pub session_id: Option<u64>,
    pub text: String,
    pub raw_text: String,
    pub level: f32,
    pub elapsed_seconds: u64,
    pub error: Option<String>,
    pub exit_intent: Option<ExitIntent>,
    pub delivery: Delivery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveAction {
    Copy,
    Dismiss,
    Confirm,
    CancelExit,
}
#[derive(Clone, Debug)]
pub enum AudioEvent {
    Text(String),
    Level(f32),
    Failed(String),
}
pub type AudioSink = Arc<dyn Fn(AudioEvent) + Send + Sync>;
/// Cancellation is safe only before native microphone startup. Once started,
/// the owning command must await startup and finish, even when stopped.
pub struct CaptureStartup(watch::Receiver<Signal>);
impl CaptureStartup {
    async fn prepare<T>(
        mut self,
        preparation: impl std::future::Future<Output = T>,
    ) -> Result<T, String> {
        let value = tokio::select! {
            biased;
            _ = self.0.wait_for(|signal| *signal != Signal::Run) => {
                return Err("Dictation stopped before microphone startup.".into());
            }
            value = preparation => value,
        };
        if *self.0.borrow() != Signal::Run {
            Err("Dictation stopped before microphone startup.".into())
        } else {
            Ok(value)
        }
    }
}
#[async_trait]
pub trait Capture: Send {
    fn cancel(&self);
    async fn finish(self: Box<Self>) -> Result<String, String>;
}
pub trait Target: Send {
    fn paste(&mut self, text: &str) -> Result<Delivery, String>;
}
/// These methods vary by OS/device/provider, rather than by test-only behavior.
#[async_trait]
pub trait Platform: Send + Sync {
    fn snapshot_target(&self) -> Result<Box<dyn Target>, String>;
    async fn start_capture(
        &self,
        microphone: Option<String>,
        sink: AudioSink,
        startup: CaptureStartup,
    ) -> Result<Box<dyn Capture>, String>;
    async fn provider_identity(&self) -> Result<String, String>;
    async fn cleanup(&self, identity: &str, text: &str) -> Result<String, String>;
    async fn apply_settings(&self, old: &SavedSettings, new: &SavedSettings) -> Result<(), String>;
    fn copy(&self, text: &str) -> Result<(), String>;
    fn desktop_available(&self) -> bool;
    fn chord_held(&self, shortcut: &str) -> bool;
    fn publish(&self, snapshot: DictationSnapshot);
    fn activity(&self, active: bool);
    fn show_review(&self);
    fn quit(&self, review_resolved: bool) -> Result<(), String>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Signal {
    Run,
    Stop,
    Interrupt,
    Cancel,
}
struct Active {
    id: u64,
    signal: watch::Sender<Signal>,
    done: watch::Receiver<bool>,
}
struct Data {
    saved: SavedSettings,
    snapshot: DictationSnapshot,
    generation: u64,
    active: Option<Active>,
    complete: bool,
    history_saved: bool,
    pending_disable: Option<SettingsInput>,
}
pub struct DictationCommands {
    data: Mutex<Data>,
    commands: AsyncMutex<()>,
    publication: Mutex<()>,
    // Suspend permanently closes a pool. Keep the owner so resume can supply
    // the replacement to every operation, including background expiry.
    database: Arc<Database>,
    platform: Arc<dyn Platform>,
    gate: CaptureGate,
}
impl DictationCommands {
    pub async fn new(
        database: Arc<Database>,
        platform: Arc<dyn Platform>,
        gate: CaptureGate,
    ) -> Result<Arc<Self>, String> {
        let pool = crate::db::ensure_pool(&database).await?;
        let saved: SavedSettings = history::load_settings(&pool).await?;
        history::expire(&pool, chrono::Utc::now()).await?;
        let snapshot = DictationSnapshot {
            settings: saved.settings.clone(),
            provider_authorized: false,
            phase: if saved.settings.enabled {
                Phase::Idle
            } else {
                Phase::Disabled
            },
            session_id: None,
            text: String::new(),
            raw_text: String::new(),
            level: 0.0,
            elapsed_seconds: 0,
            error: None,
            exit_intent: None,
            delivery: Delivery::None,
        };
        Ok(Arc::new(Self {
            data: Mutex::new(Data {
                saved,
                snapshot,
                generation: 0,
                active: None,
                complete: false,
                history_saved: false,
                pending_disable: None,
            }),
            commands: AsyncMutex::new(()),
            publication: Mutex::new(()),
            database,
            platform,
            gate,
        }))
    }
    fn snapshot(&self) -> DictationSnapshot {
        self.data.lock().unwrap().snapshot.clone()
    }
    fn publish(&self) {
        let _publication = self.publication.lock().unwrap();
        self.platform.publish(self.snapshot());
    }
    pub async fn get_dictation_state(&self) -> Result<DictationSnapshot, String> {
        let identity = self.platform.provider_identity().await.ok();
        let mut data = self.data.lock().unwrap();
        data.snapshot.provider_authorized =
            identity.is_some() && identity == data.saved.provider_identity;
        Ok(data.snapshot.clone())
    }
    pub async fn set_dictation_settings(
        self: &Arc<Self>,
        input: SettingsInput,
    ) -> Result<DictationSnapshot, String> {
        let _command = self.commands.lock().await;
        let current = self.snapshot();
        if current.phase.active() || current.phase == Phase::Review {
            if input.settings.enabled {
                return Err("Finish or resolve dictation before changing its settings.".into());
            }
            {
                let mut data = self.data.lock().unwrap();
                data.pending_disable = Some(input.clone());
                data.snapshot.exit_intent = Some(ExitIntent::Disable);
            }
            self.interrupt("Dictation was disabled. Copy or discard the available text.");
            self.wait_finished().await;
            if self.snapshot().phase == Phase::Review {
                self.publish();
                self.show_exit_review();
                return Ok(self.snapshot());
            }
        }
        self.apply_settings(input).await?;
        Ok(self.snapshot())
    }
    async fn apply_settings(&self, input: SettingsInput) -> Result<(), String> {
        if input.settings.shortcut.trim().is_empty() {
            return Err("Choose a dictation shortcut.".into());
        }
        let old = self.data.lock().unwrap().saved.clone();
        let identity = if input.consent_provider {
            Some(self.platform.provider_identity().await?)
        } else {
            old.provider_identity.clone()
        };
        let new = SavedSettings {
            settings: input.settings,
            provider_identity: identity,
        };
        self.platform.apply_settings(&old, &new).await?;
        {
            let mut data = self.data.lock().unwrap();
            data.saved = new.clone();
            data.snapshot.settings = new.settings.clone();
            data.snapshot.exit_intent = None;
            data.pending_disable = None;
            if data.snapshot.phase != Phase::Review {
                data.snapshot.phase = if new.settings.enabled {
                    Phase::Idle
                } else {
                    Phase::Disabled
                };
            }
        }
        self.get_dictation_state().await?;
        self.publish();
        Ok(())
    }
    pub async fn activate_shortcut(self: &Arc<Self>) -> Result<DictationSnapshot, String> {
        let snapshot = self.snapshot();
        if snapshot.phase.active() && snapshot.settings.activation == Activation::Toggle {
            self.stop_dictation().await
        } else {
            self.start(snapshot.settings.activation == Activation::Hold)
                .await
        }
    }
    pub async fn start_dictation(self: &Arc<Self>) -> Result<DictationSnapshot, String> {
        self.start(false).await
    }
    async fn start(self: &Arc<Self>, hold_chord: bool) -> Result<DictationSnapshot, String> {
        let _command = self.commands.lock().await;
        let current = self.snapshot();
        if !current.settings.enabled {
            return Err("Enable dictation first.".into());
        }
        if current.phase.active() {
            return Err("Dictation is already active.".into());
        }
        if current.phase == Phase::Review || current.exit_intent.is_some() {
            return Err("Copy or dismiss the previous dictation first.".into());
        }
        if !self.platform.desktop_available() {
            return Err("Unlock Windows before starting dictation.".into());
        }
        let lease = self.gate.claim(CaptureOwner::Dictation)?;
        // Capture the external editor before publishing state or showing the pill.
        // Unsupported editors still permit local transcription and explicit copy.
        let target = self.platform.snapshot_target().ok();
        let (signal, receiver) = watch::channel(Signal::Run);
        let (done, completion) = watch::channel(false);
        let (id, saved, starting) = {
            let mut data = self.data.lock().unwrap();
            data.generation = data
                .generation
                .checked_add(1)
                .ok_or("Restart the app before another dictation.")?;
            let id = data.generation;
            data.active = Some(Active {
                id,
                signal,
                done: completion,
            });
            data.complete = false;
            data.history_saved = false;
            data.snapshot.phase = Phase::Starting;
            data.snapshot.session_id = Some(id);
            data.snapshot.text.clear();
            data.snapshot.raw_text.clear();
            data.snapshot.error = None;
            data.snapshot.delivery = Delivery::None;
            data.snapshot.elapsed_seconds = 0;
            data.snapshot.level = 0.0;
            (id, data.saved.clone(), data.snapshot.clone())
        };
        self.platform.activity(true);
        self.publish();
        let commands = self.clone();
        tokio::spawn(async move {
            commands.run(id, saved, target, receiver, lease, done).await;
        });
        let commands = self.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            let mut ticker = tokio::time::interval(Duration::from_millis(40));
            loop {
                ticker.tick().await;
                let snapshot = commands.snapshot();
                if snapshot.session_id != Some(id) || !snapshot.phase.active() {
                    break;
                }
                if !commands.platform.desktop_available() {
                    commands.interrupt("Windows became unavailable. Review the available text.");
                } else if start.elapsed() >= Duration::from_secs(600)
                    && matches!(snapshot.phase, Phase::Starting | Phase::Listening)
                {
                    commands.interrupt(
                        "The ten-minute dictation limit was reached. Review the available text.",
                    );
                } else if hold_chord
                    && matches!(snapshot.phase, Phase::Starting | Phase::Listening)
                    && !commands.platform.chord_held(&snapshot.settings.shortcut)
                {
                    commands.signal(Signal::Stop);
                }
                let elapsed = start.elapsed().as_secs();
                if snapshot.elapsed_seconds != elapsed {
                    commands.update(id, |data| data.snapshot.elapsed_seconds = elapsed);
                }
            }
        });
        Ok(starting)
    }
    fn update(&self, id: u64, update: impl FnOnce(&mut Data)) {
        {
            let mut data = self.data.lock().unwrap();
            if !data
                .active
                .as_ref()
                .is_some_and(|active| active.id == id && *active.signal.borrow() != Signal::Cancel)
            {
                return;
            }
            update(&mut data);
        }
        self.publish();
    }
    fn signal(&self, signal: Signal) {
        let data = self.data.lock().unwrap();
        if let Some(active) = &data.active {
            let previous = *active.signal.borrow();
            if previous != Signal::Cancel
                && (signal == Signal::Cancel || previous != Signal::Interrupt)
            {
                active.signal.send_replace(signal);
            }
        }
    }
    pub fn interrupt(&self, reason: &str) {
        {
            let mut data = self.data.lock().unwrap();
            if data.active.is_none() {
                return;
            }
            data.snapshot.error = Some(reason.into());
            data.complete = false;
        }
        self.signal(Signal::Interrupt);
        self.publish();
    }
    async fn wait_finished(&self) {
        let done = self
            .data
            .lock()
            .unwrap()
            .active
            .as_ref()
            .map(|active| active.done.clone());
        if let Some(mut done) = done {
            let _ = done.wait_for(|finished| *finished).await;
        }
    }
    pub async fn stop_dictation(&self) -> Result<DictationSnapshot, String> {
        if !self.snapshot().phase.active() {
            return Ok(self.snapshot());
        }
        // Do not downgrade an interruption or cancellation to a normal finish.
        self.signal(Signal::Stop);
        self.wait_finished().await;
        Ok(self.snapshot())
    }
    pub async fn cancel_dictation(&self) -> Result<DictationSnapshot, String> {
        let _command = self.commands.lock().await;
        if self.snapshot().phase == Phase::Review {
            return self.resolve_review(ResolveAction::Dismiss).await;
        }
        self.signal(Signal::Cancel);
        self.wait_finished().await;
        let exit = {
            let mut data = self.data.lock().unwrap();
            let exit = data.snapshot.exit_intent;
            self.clear(&mut data);
            exit
        };
        if let Err(error) = self.complete_exit(exit, false).await {
            {
                let mut data = self.data.lock().unwrap();
                data.snapshot.exit_intent = None;
                data.snapshot.error = Some(error.clone());
            }
            self.publish();
            return Err(error);
        }
        self.publish();
        Ok(self.snapshot())
    }
    fn clear(&self, data: &mut Data) {
        data.snapshot.phase = if data.saved.settings.enabled {
            Phase::Idle
        } else {
            Phase::Disabled
        };
        data.snapshot.text.clear();
        data.snapshot.raw_text.clear();
        data.snapshot.session_id = None;
        data.snapshot.level = 0.0;
        data.snapshot.error = None;
        data.snapshot.delivery = Delivery::None;
        data.complete = false;
        self.gate.set_dictation_review(false);
    }
    async fn run(
        self: Arc<Self>,
        id: u64,
        saved: SavedSettings,
        target: Option<Box<dyn Target>>,
        mut signal: watch::Receiver<Signal>,
        lease: CaptureLease,
        done: watch::Sender<bool>,
    ) {
        let result = self.run_session(id, &saved, target, &mut signal).await;
        let cancelled = *signal.borrow() == Signal::Cancel;
        {
            let mut data = self.data.lock().unwrap();
            if data.active.as_ref().is_some_and(|active| active.id == id) {
                data.active = None;
                data.snapshot.level = 0.0;
                data.complete = result.as_ref().is_ok_and(|complete| *complete) && !cancelled;
                if let Err(error) = result {
                    if data.snapshot.error.is_none() {
                        data.snapshot.error = Some(error);
                    }
                }
                if cancelled
                    || data.snapshot.text.trim().is_empty()
                    || data.snapshot.delivery == Delivery::Verified
                {
                    let error = data.snapshot.error.clone();
                    let delivery = data.snapshot.delivery;
                    self.clear(&mut data);
                    if !cancelled {
                        data.snapshot.error = error;
                        data.snapshot.delivery = delivery;
                    }
                } else {
                    data.snapshot.phase = Phase::Review;
                    // Review must exclude updates before capture ownership is released.
                    self.gate.set_dictation_review(true);
                }
            }
        }
        drop(lease);
        self.platform.activity(false);
        done.send_replace(true);
        self.publish();
    }
    async fn run_session(
        self: &Arc<Self>,
        id: u64,
        saved: &SavedSettings,
        mut target: Option<Box<dyn Target>>,
        signal: &mut watch::Receiver<Signal>,
    ) -> Result<bool, String> {
        let commands = Arc::downgrade(self);
        let sink: AudioSink = Arc::new(move |event| {
            if let Some(commands) = commands.upgrade() {
                match event {
                    AudioEvent::Text(text) => commands.update(id, |data| {
                        data.snapshot.raw_text = text.clone();
                        data.snapshot.text = text;
                    }),
                    AudioEvent::Level(level) => commands.update(id, |data| {
                        data.snapshot.level = if level.is_finite() {
                            level.clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    }),
                    AudioEvent::Failed(reason) => {
                        if commands.snapshot().session_id == Some(id) {
                            commands.interrupt(&reason);
                        }
                    }
                }
            }
        });
        if *signal.borrow() != Signal::Run {
            return Ok(false);
        }
        // Startup and finish own native work. Dropping either future only asks
        // it to stop; it does not prove teardown. Keep this task and its lease
        // until startup resolves and any resulting capture has fully drained.
        let capture = self
            .platform
            .start_capture(
                saved.settings.microphone_id.clone(),
                sink,
                CaptureStartup(signal.clone()),
            )
            .await?;
        if *signal.borrow() == Signal::Run {
            self.update(id, |data| data.snapshot.phase = Phase::Listening);
            let _ = signal.wait_for(|signal| *signal != Signal::Run).await;
        }
        capture.cancel();
        self.update(id, |data| {
            data.snapshot.phase = Phase::Processing;
            data.snapshot.level = 0.0;
        });
        let raw = capture.finish().await?;
        self.update(id, |data| {
            data.snapshot.raw_text = raw.clone();
            data.snapshot.text = raw.clone();
        });
        if *signal.borrow() != Signal::Stop {
            return Ok(false);
        }
        if raw.trim().is_empty() {
            return Ok(true);
        }
        let text = if saved.settings.cleanup == Cleanup::Provider {
            let identity = saved
                .provider_identity
                .as_deref()
                .ok_or("Authorize this provider for dictation cleanup first.")?;
            tokio::select! {
                biased;
                _ = signal.wait_for(|signal| matches!(signal, Signal::Interrupt | Signal::Cancel)) => return Ok(false),
                text = self.platform.cleanup(identity, &raw) => text?,
            }
        } else {
            raw
        };
        if text.trim().is_empty() {
            return Err("Cleanup returned no final text. Review the raw transcript.".into());
        }
        self.update(id, |data| data.snapshot.text = text.clone());
        let delivered = {
            // This is the delivery linearization point. Cancellation before this
            // lock invalidates insertion; cancellation cannot undo native input.
            let mut data = self.data.lock().unwrap();
            if matches!(*signal.borrow(), Signal::Cancel | Signal::Interrupt) {
                return Ok(false);
            }
            if !self.platform.desktop_available() {
                return Err("Windows became unavailable. Review the available text.".into());
            }
            if saved.settings.clipboard_consent {
                if let Some(target) = target.as_mut() {
                    let outcome = target.paste(&text)?;
                    data.snapshot.delivery = outcome;
                    outcome == Delivery::Verified
                } else {
                    false
                }
            } else {
                false
            }
        };
        if delivered && saved.settings.history_enabled {
            self.save_optional_history(&text).await;
        }
        Ok(true)
    }
    pub async fn resolve_dictation(
        &self,
        action: ResolveAction,
    ) -> Result<DictationSnapshot, String> {
        let _command = self.commands.lock().await;
        self.resolve_review(action).await
    }
    async fn resolve_review(&self, action: ResolveAction) -> Result<DictationSnapshot, String> {
        let snapshot = self.snapshot();
        if snapshot.phase != Phase::Review {
            return Err("There is no dictation to review.".into());
        }
        if action == ResolveAction::CancelExit {
            let mut data = self.data.lock().unwrap();
            data.snapshot.exit_intent = None;
            data.pending_disable = None;
            drop(data);
            self.publish();
            return Ok(self.snapshot());
        }
        if action == ResolveAction::Confirm
            && (snapshot.delivery != Delivery::Uncertain || !self.data.lock().unwrap().complete)
        {
            return Err("Only a completed uncertain insertion can be confirmed.".into());
        }
        if action == ResolveAction::Copy {
            self.platform.copy(&snapshot.text)?;
        }
        if matches!(action, ResolveAction::Copy | ResolveAction::Confirm)
            && snapshot.settings.history_enabled
            && {
                let data = self.data.lock().unwrap();
                data.complete && !data.history_saved
            }
        {
            let saved = self.save_optional_history(&snapshot.text).await;
            self.data.lock().unwrap().history_saved = saved;
        }
        let complete = self.data.lock().unwrap().complete;
        // Keep review until the exit succeeds. Quit consumes it atomically with
        // ownership; failed Disable settings must not admit Update either.
        if let Err(error) = self
            .complete_exit(
                snapshot.exit_intent,
                snapshot.exit_intent == Some(ExitIntent::Quit),
            )
            .await
        {
            {
                let mut data = self.data.lock().unwrap();
                data.snapshot = snapshot;
                data.snapshot.error = Some(error.clone());
                data.complete = complete;
                self.gate.set_dictation_review(true);
            }
            self.publish();
            return Err(error);
        }
        self.clear(&mut self.data.lock().unwrap());
        self.publish();
        Ok(self.snapshot())
    }
    async fn complete_exit(
        &self,
        exit: Option<ExitIntent>,
        review_resolved: bool,
    ) -> Result<(), String> {
        match exit {
            Some(ExitIntent::Disable) => {
                let pending = self.data.lock().unwrap().pending_disable.clone();
                if let Some(pending) = pending {
                    self.apply_settings(pending).await?;
                }
            }
            Some(ExitIntent::Quit) => self.platform.quit(review_resolved)?,
            None => {}
        }
        self.data.lock().unwrap().snapshot.exit_intent = None;
        Ok(())
    }
    pub async fn request_quit(&self) -> Result<(), String> {
        let _command = self.commands.lock().await;
        {
            let mut data = self.data.lock().unwrap();
            data.snapshot.exit_intent = Some(ExitIntent::Quit);
            data.pending_disable = None;
        }
        self.interrupt("Quit requested. Copy or discard the available text.");
        self.wait_finished().await;
        if self.snapshot().phase == Phase::Review {
            self.publish();
            self.show_exit_review();
        } else if let Err(error) = self.platform.quit(false) {
            {
                let mut data = self.data.lock().unwrap();
                data.snapshot.exit_intent = None;
                data.snapshot.error = Some(error.clone());
            }
            self.publish();
            return Err(error);
        }
        Ok(())
    }
    fn show_exit_review(&self) {
        let snapshot = self.snapshot();
        if snapshot.phase == Phase::Review
            && snapshot.exit_intent.is_some()
            && self.platform.desktop_available()
        {
            self.platform.show_review();
        }
    }
    // History is optional and cannot undo a successful Copy or delivery. Keep
    // storage failures out of the command result and never log dictated text or
    // database errors, which may contain sensitive paths or values.
    async fn save_optional_history(&self, text: &str) -> bool {
        let result = async {
            let pool = crate::db::ensure_pool(&self.database).await?;
            history::append(&pool, text, chrono::Utc::now()).await
        }
        .await;
        if result.is_err() {
            eprintln!("[dictation] warning: could not save optional history for completed text");
        }
        result.is_ok()
    }
    pub async fn list_dictation_history(&self) -> Result<Vec<HistoryEntry>, String> {
        let pool = crate::db::ensure_pool(&self.database).await?;
        history::list(&pool, chrono::Utc::now()).await
    }
    pub async fn delete_dictation_history(&self, id: Option<i64>) -> Result<(), String> {
        let pool = crate::db::ensure_pool(&self.database).await?;
        history::delete(&pool, id).await
    }
    async fn expire_history(&self) -> Result<(), String> {
        let pool = crate::db::ensure_pool(&self.database).await?;
        history::expire(&pool, chrono::Utc::now()).await
    }
}

/// The OS binding boundary. Persistence is real SQLite in production and tests.
trait BindingRegistry: Send + Sync {
    fn key(&self, value: &str) -> Result<String, String>;
    fn check_conflict(&self, value: &str) -> Result<(), String>;
    fn current(&self) -> Option<String>;
    fn publish(&self, value: Option<String>);
    fn register(&self, value: &str) -> Result<(), String>;
    fn unregister(&self, value: &str) -> Result<(), String>;
}
async fn replace_binding(
    database: &Database,
    registry: &dyn BindingRegistry,
    changes: &AsyncMutex<Vec<String>>,
    new: &SavedSettings,
) -> Result<(), String> {
    let mut pending = changes.lock().await;
    let pool = crate::db::ensure_pool(database).await?;
    while let Some(value) = pending.last() {
        registry.unregister(value).map_err(|error| format!("{error} A previous dictation shortcut remains reserved but inactive. Retry to release it."))?;
        pending.pop();
    }
    let previous = registry.current();
    let candidate = new.settings.enabled.then(|| new.settings.shortcut.clone());
    let old_key = previous
        .as_deref()
        .map(|value| registry.key(value))
        .transpose()?;
    let new_key = candidate
        .as_deref()
        .map(|value| registry.key(value))
        .transpose()?;
    if let Some(value) = &candidate {
        registry.check_conflict(value)?;
    }
    let stored: SavedSettings = history::load_settings(&pool).await?;
    let changed = old_key != new_key;
    if changed {
        if let Some(value) = &candidate {
            registry.register(value)?;
            pending.push(value.clone());
        }
    }
    // Keep the old binding active until persistence succeeds. Retire it only
    // after saving; no await occurs between retirement and publishing authority.
    let mut failure = history::save_settings(&pool, new).await.err();
    if failure.is_none() && changed {
        if let Some(value) = &previous {
            failure = registry.unregister(value).err();
        }
    }
    if let Some(mut failure) = failure {
        while let Some(value) = pending.last() {
            if let Err(error) = registry.unregister(value) {
                failure.push_str(&format!(
                    " {error} The replacement remains reserved but inactive. Retry to release it."
                ));
                break;
            }
            pending.pop();
        }
        // Restore the entire settings value. Failed acknowledgement can still
        // mean a committed write; never assume that an error implies rollback.
        let restored = match history::load_settings::<SavedSettings>(&pool).await {
            Ok(actual) if actual == stored => Ok(()),
            _ => history::save_settings(&pool, &stored).await,
        };
        let verified = history::load_settings::<SavedSettings>(&pool).await;
        if restored.is_err() || !verified.is_ok_and(|actual| actual == stored) {
            failure.push_str(" Rollback could not restore saved dictation settings. Active settings are unchanged; restart may load different settings.");
        }
        return Err(failure);
    }
    registry.publish(candidate);
    pending.clear();
    Ok(())
}

// BEGIN TAURI ADAPTER
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState as KeyState};

pub struct DictationState {
    commands: Arc<DictationCommands>,
    key_down: std::sync::atomic::AtomicBool,
}
/// Provider access is independent of windows, capture and shortcut registration.
struct ConfiguredProvider {
    database: Arc<Database>,
}
impl ConfiguredProvider {
    async fn identity(&self) -> Result<String, String> {
        let pool = crate::db::ensure_pool(&self.database).await?;
        crate::llm::dictation_provider_identity(&pool).await
    }
    async fn cleanup(&self, identity: &str, text: &str) -> Result<String, String> {
        let pool = crate::db::ensure_pool(&self.database).await?;
        crate::llm::cleanup_dictation(&pool, identity, text).await
    }
}
struct NativePlatform {
    app: AppHandle,
    database: Arc<Database>,
    provider: ConfiguredProvider,
    binding_changes: AsyncMutex<Vec<String>>,
    interrupts_ready: std::sync::atomic::AtomicBool,
}
struct NativeCapture(crate::dictation_capture::DictationCapture);
#[async_trait]
impl Capture for NativeCapture {
    fn cancel(&self) {
        self.0.cancel();
    }
    async fn finish(self: Box<Self>) -> Result<String, String> {
        self.0.finish().await
    }
}
struct NativeTarget(crate::dictation_native::TargetSnapshot);
impl Target for NativeTarget {
    fn paste(&mut self, text: &str) -> Result<Delivery, String> {
        crate::dictation_native::paste_to_target(&self.0, text).map(|outcome| match outcome {
            crate::dictation_native::DeliveryOutcome::Verified => Delivery::Verified,
            crate::dictation_native::DeliveryOutcome::Uncertain => Delivery::Uncertain,
        })
    }
}
fn parsed(shortcut: &str) -> Result<Shortcut, String> {
    shortcut
        .parse()
        .map_err(|_| "Invalid dictation shortcut.".into())
}

struct NativeBindings<'a>(&'a AppHandle);
impl BindingRegistry for NativeBindings<'_> {
    fn key(&self, value: &str) -> Result<String, String> {
        Ok(parsed(value)?.id().to_string())
    }
    fn check_conflict(&self, value: &str) -> Result<(), String> {
        let state = self.0.state::<crate::shortcuts::ShortcutState>();
        let key = parsed(value)?;
        if state
            .current
            .lock()
            .unwrap()
            .as_deref()
            .and_then(|value| parsed(value).ok())
            .is_some_and(|meeting| meeting.id() == key.id())
        {
            return Err("This shortcut is assigned to Meetings. Choose another shortcut.".into());
        }
        Ok(())
    }
    fn current(&self) -> Option<String> {
        self.0
            .state::<crate::shortcuts::ShortcutState>()
            .dictation_current
            .lock()
            .unwrap()
            .clone()
    }
    fn publish(&self, value: Option<String>) {
        *self
            .0
            .state::<crate::shortcuts::ShortcutState>()
            .dictation_current
            .lock()
            .unwrap() = value;
    }
    fn register(&self, value: &str) -> Result<(), String> {
        self.0
            .global_shortcut()
            .register(parsed(value)?)
            .map_err(|_| {
                "The dictation shortcut is unavailable. The previous shortcut is unchanged.".into()
            })
    }
    fn unregister(&self, value: &str) -> Result<(), String> {
        self.0
            .global_shortcut()
            .unregister(parsed(value)?)
            .map_err(|_| "Could not release a dictation shortcut.".into())
    }
}

#[async_trait]
impl Platform for NativePlatform {
    fn snapshot_target(&self) -> Result<Box<dyn Target>, String> {
        Ok(Box::new(NativeTarget(
            crate::dictation_native::snapshot_target()?,
        )))
    }
    async fn start_capture(
        &self,
        microphone: Option<String>,
        sink: AudioSink,
        startup: CaptureStartup,
    ) -> Result<Box<dyn Capture>, String> {
        let engine = startup
            .prepare(crate::recording::shared_asr_engine(&self.app))
            .await?
            .ok_or("Install the speech model before dictating.")?;
        let capture = crate::dictation_capture::DictationCapture::start(
            engine,
            microphone,
            Arc::new(move |event| {
                sink(match event {
                    crate::dictation_capture::CaptureEvent::Text(text) => AudioEvent::Text(text),
                    crate::dictation_capture::CaptureEvent::Level(level) => {
                        AudioEvent::Level(level)
                    }
                    crate::dictation_capture::CaptureEvent::Failed(error) => {
                        AudioEvent::Failed(error)
                    }
                });
            }),
        )
        .await?;
        Ok(Box::new(NativeCapture(capture)))
    }
    async fn provider_identity(&self) -> Result<String, String> {
        self.provider.identity().await
    }
    async fn cleanup(&self, identity: &str, text: &str) -> Result<String, String> {
        self.provider.cleanup(identity, text).await
    }
    async fn apply_settings(
        &self,
        _old: &SavedSettings,
        new: &SavedSettings,
    ) -> Result<(), String> {
        if new.settings.enabled
            && !self
                .interrupts_ready
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(
                "Windows lock monitoring is unavailable. Reopen the app before enabling dictation."
                    .into(),
            );
        }
        let state = self.app.state::<crate::shortcuts::ShortcutState>();
        let _coordination = state.coordination.lock().await;
        replace_binding(
            &self.database,
            &NativeBindings(&self.app),
            &self.binding_changes,
            new,
        )
        .await
    }
    fn copy(&self, text: &str) -> Result<(), String> {
        crate::dictation_native::copy_text(text)
    }
    fn desktop_available(&self) -> bool {
        crate::dictation_native::desktop_is_available()
    }
    fn chord_held(&self, shortcut: &str) -> bool {
        parsed(shortcut).is_ok_and(|key| crate::dictation_native::chord_is_held(&key))
    }
    fn publish(&self, snapshot: DictationSnapshot) {
        let _ = self.app.emit("dictation:state", &snapshot);
        sync_overlay(&self.app, &snapshot);
    }
    fn activity(&self, active: bool) {
        crate::meeting_presence::recording_activity_changed(&self.app, active);
    }
    fn show_review(&self) {
        if crate::dictation_native::desktop_is_available() {
            // The nonactivating pill cannot host an accessible exit decision.
            if let Some(main) = self.app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.unminimize();
                let _ = main.set_focus();
            }
            let _ = self.app.emit("dictation:review", ());
        }
    }
    fn quit(&self, review_resolved: bool) -> Result<(), String> {
        crate::meeting_presence::quit_when_idle(&self.app, review_resolved)
    }
}

fn sync_overlay(app: &AppHandle, snapshot: &DictationSnapshot) {
    let Some(window) = app.get_webview_window("dictation") else {
        return;
    };
    if !snapshot.settings.enabled || !crate::dictation_native::desktop_is_available() {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        }
        return;
    }
    // Backend owns the 64x220 logical-pixel screen-edge pill. The overlay never
    // accepts focus. Full recovery and copy/confirm controls live in Settings.
    if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
        let logical: tauri::LogicalSize<f64> = size.to_logical(scale);
        if (logical.width - 64.0).abs() > 1.0 || (logical.height - 220.0).abs() > 1.0 {
            let _ = window.set_size(tauri::LogicalSize::new(64.0, 220.0));
        }
    }
    if let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) {
        let area = monitor.work_area();
        let margin = (16.0 * monitor.scale_factor()).round() as i32;
        let x = match snapshot.settings.side {
            Side::Left => area.position.x + margin,
            Side::Right => area.position.x + area.size.width as i32 - size.width as i32 - margin,
        };
        let y = area.position.y + (area.size.height as i32 - size.height as i32) / 2;
        let position = tauri::PhysicalPosition::new(x, y);
        if window.outer_position().ok() != Some(position) {
            let _ = window.set_position(position);
        }
    }
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
}

pub async fn request_quit(app: AppHandle) {
    if let Some(state) = app.try_state::<DictationState>() {
        if let Err(error) = state.commands.request_quit().await {
            let mut data = state.commands.data.lock().unwrap();
            data.snapshot.error = Some(error);
        }
        state.commands.publish();
    } else if let Err(error) = crate::meeting_presence::quit_when_idle(&app, false) {
        let _ = app.emit("dictation:quit_blocked", error);
    }
}
#[tauri::command]
pub async fn get_dictation_state(
    state: State<'_, DictationState>,
) -> Result<DictationSnapshot, String> {
    state.commands.get_dictation_state().await
}
#[tauri::command]
pub async fn set_dictation_settings(
    state: State<'_, DictationState>,
    input: SettingsInput,
) -> Result<DictationSnapshot, String> {
    let commands = state.commands.clone();
    // Own native registration + compensation even if the IPC client disappears.
    tauri::async_runtime::spawn(async move { commands.set_dictation_settings(input).await })
        .await
        .map_err(|_| "Dictation settings task failed.".to_string())?
}
#[tauri::command]
pub async fn start_dictation(
    state: State<'_, DictationState>,
) -> Result<DictationSnapshot, String> {
    state.commands.start_dictation().await
}
#[tauri::command]
pub async fn stop_dictation(state: State<'_, DictationState>) -> Result<DictationSnapshot, String> {
    state.commands.stop_dictation().await
}
#[tauri::command]
pub async fn cancel_dictation(
    state: State<'_, DictationState>,
) -> Result<DictationSnapshot, String> {
    state.commands.cancel_dictation().await
}
#[tauri::command]
pub async fn resolve_dictation(
    state: State<'_, DictationState>,
    action: ResolveAction,
) -> Result<DictationSnapshot, String> {
    let commands = state.commands.clone();
    tauri::async_runtime::spawn(async move { commands.resolve_dictation(action).await })
        .await
        .map_err(|_| "Dictation review task failed.".to_string())?
}
#[tauri::command]
pub async fn list_dictation_microphones(
) -> Result<Vec<crate::dictation_capture::MicrophoneOption>, String> {
    tauri::async_runtime::spawn_blocking(crate::dictation_capture::list_microphones)
        .await
        .map_err(|_| "Could not enumerate microphones.".to_string())?
}
#[tauri::command]
pub async fn list_dictation_history(
    state: State<'_, DictationState>,
) -> Result<Vec<HistoryEntry>, String> {
    state.commands.list_dictation_history().await
}
#[tauri::command]
pub async fn delete_dictation_history(
    state: State<'_, DictationState>,
    id: Option<i64>,
) -> Result<(), String> {
    state.commands.delete_dictation_history(id).await
}

pub fn handle_shortcut(app: &AppHandle, shortcut: &Shortcut, event: KeyState) -> bool {
    let Some(state) = app.try_state::<DictationState>() else {
        return false;
    };
    let bindings = app.state::<crate::shortcuts::ShortcutState>();
    if !bindings
        .dictation_current
        .lock()
        .unwrap()
        .as_deref()
        .and_then(|key| parsed(key).ok())
        .is_some_and(|key| key.id() == shortcut.id())
    {
        return false;
    }
    if event == KeyState::Released {
        state
            .key_down
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if state.commands.snapshot().settings.activation == Activation::Hold {
            state.commands.signal(Signal::Stop);
        }
        return true;
    }
    if state
        .key_down
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return true;
    }
    let commands = state.commands.clone();
    tauri::async_runtime::spawn(async move {
        let result = commands.activate_shortcut().await;
        if let Err(error) = result {
            commands.data.lock().unwrap().snapshot.error = Some(error);
            commands.publish();
        }
    });
    true
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    tauri::async_runtime::block_on(async {
        let database = handle.state::<crate::db::DbState>().pool.clone();
        let platform = Arc::new(NativePlatform {
            app: handle.clone(),
            database: database.clone(),
            provider: ConfiguredProvider {
                database: database.clone(),
            },
            binding_changes: AsyncMutex::new(Vec::new()),
            interrupts_ready: std::sync::atomic::AtomicBool::new(false),
        });
        let commands = DictationCommands::new(
            database,
            platform.clone(),
            handle.state::<CaptureGate>().inner().clone(),
        )
        .await?;
        handle.manage(DictationState {
            commands: commands.clone(),
            key_down: std::sync::atomic::AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&commands);
        let monitor = crate::dictation_native::start_interrupt_monitor(Arc::new(move || {
            if let Some(commands) = weak.upgrade() {
                commands.interrupt("Windows locked or suspended. Review the available text.");
            }
        }));
        let saved = commands.data.lock().unwrap().saved.clone();
        let registered = match monitor {
            Ok(()) => { platform.interrupts_ready.store(true, std::sync::atomic::Ordering::SeqCst); platform.apply_settings(&SavedSettings::default(), &saved).await },
            Err(_) => Err("Dictation could not monitor Windows lock and sleep. Reopen the app before enabling dictation.".into()),
        };
        if let Err(error) = registered {
            let mut data = commands.data.lock().unwrap();
            data.saved.settings.enabled = false;
            data.snapshot.settings.enabled = false;
            data.snapshot.phase = Phase::Disabled;
            data.snapshot.error = Some(error);
        }
        commands.get_dictation_state().await?;
        commands.publish();
        let app = handle.clone();
        // Keep locked review hidden, reveal it without activating on unlock,
        // and release key-repeat suppression when ANY chord key is released.
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(100));
            let mut expiry = Instant::now();
            let mut desktop_available = crate::dictation_native::desktop_is_available();
            loop {
                ticker.tick().await;
                let state = app.state::<DictationState>();
                let snapshot = state.commands.snapshot();
                if !state
                    .commands
                    .platform
                    .chord_held(&snapshot.settings.shortcut)
                {
                    state
                        .key_down
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                }
                sync_overlay(&app, &snapshot);
                let available = state.commands.platform.desktop_available();
                if available && !desktop_available {
                    state.commands.show_exit_review();
                }
                desktop_available = available;
                if expiry.elapsed() >= Duration::from_secs(60) {
                    if state.commands.expire_history().await.is_err() {
                        eprintln!("[dictation] could not expire history");
                    }
                    expiry = Instant::now();
                }
            }
        });
        Ok::<(), String>(())
    })?;
    Ok(())
}
// END TAURI ADAPTER

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct FakePlatform {
        scratch: Arc<ScratchDatabase>,
        pool: sqlx::SqlitePool,
        provider: ConfiguredProvider,
        use_configured_provider: AtomicBool,
        identity: Mutex<String>,
        sink: Mutex<Option<AudioSink>>,
        pasted: Arc<Mutex<Vec<String>>>,
        copied: Mutex<Vec<String>>,
        copy_error: AtomicBool,
        capture_starts: AtomicUsize,
        cleanup_started: AtomicBool,
        cleanup_release: tokio::sync::Notify,
        slow_cleanup: AtomicBool,
        cleanup_error: AtomicBool,
        held: AtomicBool,
        desktop: AtomicBool,
        outcome: Mutex<Delivery>,
        events: Mutex<Vec<DictationSnapshot>>,
        quits: AtomicUsize,
        review_shown: AtomicUsize,
        gate: CaptureGate,
        race_update: AtomicBool,
        update_claim: Mutex<Option<CaptureLease>>,
        quit_claim: Mutex<Option<CaptureLease>>,
        bindings: FakeBindings,
        binding_changes: AsyncMutex<Vec<String>>,
        slow_prepare: AtomicBool,
        preparing: AtomicBool,
        prepare_release: tokio::sync::Notify,
        slow_start: AtomicBool,
        start_release: tokio::sync::Notify,
        slow_finish: AtomicBool,
        finish_error: AtomicBool,
        finish_text: Mutex<String>,
        finish_started: Arc<AtomicBool>,
        finish_release: Arc<tokio::sync::Notify>,
        capture_finished: Arc<AtomicBool>,
        capture_stopped: Arc<AtomicBool>,
        microphone: Mutex<Option<String>>,
    }
    #[derive(Default)]
    struct FakeBindings {
        current: Mutex<Option<String>>,
        registered: Mutex<std::collections::HashSet<String>>,
        fail_unregister: AtomicBool,
        meeting: Mutex<Option<String>>,
    }
    impl BindingRegistry for FakeBindings {
        fn key(&self, value: &str) -> Result<String, String> {
            Ok(value.to_lowercase().replace("control", "ctrl"))
        }
        fn check_conflict(&self, value: &str) -> Result<(), String> {
            if self
                .meeting
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|meeting| self.key(meeting).unwrap() == self.key(value).unwrap())
            {
                return Err("Meeting shortcut conflict.".into());
            }
            Ok(())
        }
        fn current(&self) -> Option<String> {
            self.current.lock().unwrap().clone()
        }
        fn publish(&self, value: Option<String>) {
            *self.current.lock().unwrap() = value;
        }
        fn register(&self, value: &str) -> Result<(), String> {
            if !self.registered.lock().unwrap().insert(self.key(value)?) {
                return Err("OS shortcut conflict.".into());
            }
            Ok(())
        }
        fn unregister(&self, value: &str) -> Result<(), String> {
            if self.fail_unregister.load(Ordering::SeqCst) {
                return Err("OS unregister failed.".into());
            }
            self.registered.lock().unwrap().remove(&self.key(value)?);
            Ok(())
        }
    }
    struct FakeCapture {
        slow_finish: bool,
        finish_error: bool,
        finish_text: String,
        sink: AudioSink,
        finish_started: Arc<AtomicBool>,
        finish_release: Arc<tokio::sync::Notify>,
        finished: Arc<AtomicBool>,
        stopped: Arc<AtomicBool>,
    }
    impl Drop for FakeCapture {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl Capture for FakeCapture {
        fn cancel(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
        async fn finish(self: Box<Self>) -> Result<String, String> {
            self.cancel();
            self.finish_started.store(true, Ordering::SeqCst);
            if self.slow_finish {
                self.finish_release.notified().await;
            }
            (self.sink)(AudioEvent::Text(self.finish_text.clone()));
            self.finished.store(true, Ordering::SeqCst);
            if self.finish_error {
                Err("Microphone disconnected during finalization.".into())
            } else {
                Ok(self.finish_text.clone())
            }
        }
    }
    struct FakeTarget {
        pasted: Arc<Mutex<Vec<String>>>,
        outcome: Delivery,
    }
    impl Target for FakeTarget {
        fn paste(&mut self, text: &str) -> Result<Delivery, String> {
            self.pasted.lock().unwrap().push(text.into());
            Ok(self.outcome)
        }
    }
    #[async_trait]
    impl Platform for FakePlatform {
        fn snapshot_target(&self) -> Result<Box<dyn Target>, String> {
            Ok(Box::new(FakeTarget {
                pasted: self.pasted.clone(),
                outcome: *self.outcome.lock().unwrap(),
            }))
        }
        async fn start_capture(
            &self,
            mic: Option<String>,
            sink: AudioSink,
            startup: CaptureStartup,
        ) -> Result<Box<dyn Capture>, String> {
            startup
                .prepare(async {
                    if self.slow_prepare.load(Ordering::SeqCst) {
                        self.preparing.store(true, Ordering::SeqCst);
                        self.prepare_release.notified().await;
                    }
                })
                .await?;
            self.capture_starts.fetch_add(1, Ordering::SeqCst);
            *self.microphone.lock().unwrap() = mic;
            if self.slow_start.load(Ordering::SeqCst) {
                self.start_release.notified().await;
            }
            *self.sink.lock().unwrap() = Some(sink.clone());
            sink(AudioEvent::Text("raw words".into()));
            Ok(Box::new(FakeCapture {
                slow_finish: self.slow_finish.load(Ordering::SeqCst),
                finish_error: self.finish_error.load(Ordering::SeqCst),
                finish_text: self.finish_text.lock().unwrap().clone(),
                sink,
                finish_started: self.finish_started.clone(),
                finish_release: self.finish_release.clone(),
                finished: self.capture_finished.clone(),
                stopped: self.capture_stopped.clone(),
            }))
        }
        async fn provider_identity(&self) -> Result<String, String> {
            if self.use_configured_provider.load(Ordering::SeqCst) {
                return self.provider.identity().await;
            }
            Ok(self.identity.lock().unwrap().clone())
        }
        async fn cleanup(&self, identity: &str, text: &str) -> Result<String, String> {
            if self.use_configured_provider.load(Ordering::SeqCst) {
                return self.provider.cleanup(identity, text).await;
            }
            if identity != *self.identity.lock().unwrap() {
                return Err("Provider consent changed.".into());
            }
            self.cleanup_started.store(true, Ordering::SeqCst);
            if self.slow_cleanup.load(Ordering::SeqCst) {
                self.cleanup_release.notified().await;
            }
            if self.cleanup_error.load(Ordering::SeqCst) {
                Err("Cleanup failed.".into())
            } else {
                Ok("Clean words.".into())
            }
        }
        async fn apply_settings(
            &self,
            _old: &SavedSettings,
            new: &SavedSettings,
        ) -> Result<(), String> {
            replace_binding(
                &self.scratch.database,
                &self.bindings,
                &self.binding_changes,
                new,
            )
            .await
        }
        fn copy(&self, text: &str) -> Result<(), String> {
            if self.copy_error.load(Ordering::SeqCst) {
                return Err("Clipboard unavailable.".into());
            }
            self.copied.lock().unwrap().push(text.into());
            Ok(())
        }
        fn desktop_available(&self) -> bool {
            self.desktop.load(Ordering::SeqCst)
        }
        fn chord_held(&self, _shortcut: &str) -> bool {
            self.held.load(Ordering::SeqCst)
        }
        fn publish(&self, state: DictationSnapshot) {
            self.events.lock().unwrap().push(state);
        }
        fn activity(&self, _active: bool) {}
        fn show_review(&self) {
            self.review_shown.fetch_add(1, Ordering::SeqCst);
        }
        fn quit(&self, review_resolved: bool) -> Result<(), String> {
            if self.race_update.load(Ordering::SeqCst) {
                *self.update_claim.lock().unwrap() = self.gate.claim(CaptureOwner::Update).ok();
            }
            *self.quit_claim.lock().unwrap() = Some(if review_resolved {
                self.gate.claim_quit_after_review_resolved()?
            } else {
                self.gate.claim(CaptureOwner::Quit)?
            });
            self.quits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    async fn fixture() -> (Arc<DictationCommands>, Arc<FakePlatform>) {
        fixture_with_gate(CaptureGate::default()).await
    }
    async fn fixture_with_gate(gate: CaptureGate) -> (Arc<DictationCommands>, Arc<FakePlatform>) {
        fixture_with_database(gate, Arc::new(ScratchDatabase::new())).await
    }
    async fn fixture_with_database(
        gate: CaptureGate,
        scratch: Arc<ScratchDatabase>,
    ) -> (Arc<DictationCommands>, Arc<FakePlatform>) {
        let pool = scratch.database.pool().await.unwrap();
        let platform = Arc::new(FakePlatform {
            scratch: scratch.clone(),
            pool: pool.clone(),
            provider: ConfiguredProvider {
                database: scratch.database.clone(),
            },
            use_configured_provider: AtomicBool::new(false),
            identity: Mutex::new("openai|https://one.test/v1".into()),
            sink: Mutex::new(None),
            pasted: Arc::new(Mutex::new(vec![])),
            copied: Mutex::new(vec![]),
            copy_error: AtomicBool::new(false),
            capture_starts: AtomicUsize::new(0),
            cleanup_started: AtomicBool::new(false),
            cleanup_release: tokio::sync::Notify::new(),
            slow_cleanup: AtomicBool::new(false),
            cleanup_error: AtomicBool::new(false),
            held: AtomicBool::new(true),
            desktop: AtomicBool::new(true),
            outcome: Mutex::new(Delivery::Verified),
            events: Mutex::new(vec![]),
            quits: AtomicUsize::new(0),
            review_shown: AtomicUsize::new(0),
            gate: gate.clone(),
            race_update: AtomicBool::new(false),
            update_claim: Mutex::new(None),
            quit_claim: Mutex::new(None),
            bindings: FakeBindings::default(),
            binding_changes: AsyncMutex::new(vec![]),
            slow_prepare: AtomicBool::new(false),
            preparing: AtomicBool::new(false),
            prepare_release: tokio::sync::Notify::new(),
            slow_start: AtomicBool::new(false),
            start_release: tokio::sync::Notify::new(),
            slow_finish: AtomicBool::new(false),
            finish_error: AtomicBool::new(false),
            finish_text: Mutex::new("raw words".into()),
            finish_started: Arc::new(AtomicBool::new(false)),
            finish_release: Arc::new(tokio::sync::Notify::new()),
            capture_finished: Arc::new(AtomicBool::new(false)),
            capture_stopped: Arc::new(AtomicBool::new(false)),
            microphone: Mutex::new(None),
        });
        let commands = DictationCommands::new(scratch.database.clone(), platform.clone(), gate)
            .await
            .unwrap();
        (commands, platform)
    }
    async fn enable(commands: &Arc<DictationCommands>, cleanup: Cleanup, consent: bool) {
        let settings = DictationSettings {
            enabled: true,
            activation: Activation::Toggle,
            cleanup,
            clipboard_consent: consent,
            history_enabled: true,
            ..Default::default()
        };
        commands
            .set_dictation_settings(SettingsInput {
                settings,
                consent_provider: true,
            })
            .await
            .unwrap();
    }
    async fn wait_until(ready: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("capture boundary timed out");
    }
    async fn wait_phase(commands: &DictationCommands, phase: Phase) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if commands.get_dictation_state().await.unwrap().phase == phase {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("phase transition timed out");
    }
    struct ScratchDatabase {
        database: Arc<crate::db_safety::Database>,
        root: std::path::PathBuf,
    }
    impl ScratchDatabase {
        fn new() -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::var_os("TMPDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(format!(
                    "dictation-resume-{}-{}-{}",
                    std::process::id(),
                    chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                    NEXT_ID.fetch_add(1, Ordering::SeqCst)
                ));
            std::fs::create_dir_all(&root).unwrap();
            Self {
                database: Arc::new(crate::db_safety::Database::new(Ok(root.join("test.db")))),
                root,
            }
        }
    }
    impl Drop for ScratchDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[derive(Clone, Copy, Debug)]
    enum HistoryFault {
        Unavailable,
        Append,
        Expire,
    }
    impl HistoryFault {
        async fn inject(self, platform: &FakePlatform) {
            if matches!(self, Self::Unavailable) {
                platform.scratch.database.suspend().await.unwrap();
                return;
            }
            let event = if matches!(self, Self::Append) {
                "INSERT"
            } else {
                // Seed an expired entry so append's expiry really executes DELETE.
                sqlx::query(
                    "INSERT INTO dictation_history(text, created_at) VALUES ('expired', ?)",
                )
                .bind((chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339())
                .execute(&platform.pool)
                .await
                .unwrap();
                "DELETE"
            };
            sqlx::raw_sql(&format!(
                "CREATE TRIGGER reject_history BEFORE {event} ON dictation_history \
                 BEGIN SELECT RAISE(FAIL, 'synthetic history storage failure'); END;"
            ))
            .execute(&platform.pool)
            .await
            .unwrap();
        }
        async fn restore(self, platform: &FakePlatform) {
            if matches!(self, Self::Unavailable) {
                platform.scratch.database.resume().await;
            } else {
                sqlx::raw_sql("DROP TRIGGER reject_history;")
                    .execute(&platform.pool)
                    .await
                    .unwrap();
            }
        }
    }
    async fn completed_review(outcome: Delivery) -> (Arc<DictationCommands>, Arc<FakePlatform>) {
        let (commands, platform) = fixture().await;
        *platform.outcome.lock().unwrap() = outcome;
        enable(&commands, Cleanup::Raw, outcome != Delivery::None).await;
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        let review = commands.stop_dictation().await.unwrap();
        assert_eq!(review.phase, Phase::Review);
        assert_eq!(review.text, "raw words");
        assert_eq!(review.delivery, outcome);
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        (commands, platform)
    }
    async fn request_review_exit(commands: &Arc<DictationCommands>, exit: Option<ExitIntent>) {
        match exit {
            Some(ExitIntent::Quit) => commands.request_quit().await.unwrap(),
            Some(ExitIntent::Disable) => {
                let mut settings = commands.get_dictation_state().await.unwrap().settings;
                settings.enabled = false;
                commands
                    .set_dictation_settings(SettingsInput {
                        settings,
                        consent_provider: false,
                    })
                    .await
                    .unwrap();
            }
            None => {}
        }
        assert_eq!(
            commands.get_dictation_state().await.unwrap().exit_intent,
            exit
        );
    }
    async fn resolve_with_history_failure(
        action: ResolveAction,
        exit: Option<ExitIntent>,
        fault: HistoryFault,
    ) {
        let outcome = if action == ResolveAction::Confirm {
            Delivery::Uncertain
        } else {
            Delivery::None
        };
        let (commands, platform) = completed_review(outcome).await;
        request_review_exit(&commands, exit).await;
        fault.inject(&platform).await;
        // The real gate must remain closed until Quit consumes review atomically.
        platform.race_update.store(true, Ordering::SeqCst);
        let result = commands.resolve_dictation(action).await;
        assert_eq!(
            *platform.copied.lock().unwrap(),
            if action == ResolveAction::Copy {
                vec!["raw words"]
            } else {
                vec![]
            }
        );
        let resolved = result.unwrap_or_else(|error| {
            panic!("{fault:?} history failure must not fail {action:?}/{exit:?}: {error}")
        });
        assert_eq!(
            resolved.phase,
            if exit == Some(ExitIntent::Disable) {
                Phase::Disabled
            } else {
                Phase::Idle
            }
        );
        assert!(resolved.text.is_empty());
        assert!(resolved.raw_text.is_empty());
        assert!(resolved.error.is_none());
        assert_eq!(resolved.exit_intent, None);
        assert!(!platform.gate.has_dictation_review());
        assert_eq!(
            platform.quits.load(Ordering::SeqCst),
            usize::from(exit == Some(ExitIntent::Quit))
        );
        assert!(platform.update_claim.lock().unwrap().is_none());
        if exit == Some(ExitIntent::Quit) {
            assert!(platform.gate.claim(CaptureOwner::Update).is_err());
            assert!(platform.gate.claim(CaptureOwner::Meeting).is_err());
        } else {
            assert!(platform.gate.claim(CaptureOwner::Update).is_ok());
        }
        // A redundant resolution must not copy or paste again.
        assert!(commands.resolve_dictation(action).await.is_err());
        assert_eq!(
            platform.copied.lock().unwrap().len(),
            usize::from(action == ResolveAction::Copy)
        );
        assert_eq!(
            platform.pasted.lock().unwrap().len(),
            usize::from(action == ResolveAction::Confirm)
        );
        fault.restore(&platform).await;
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        if exit == Some(ExitIntent::Disable) {
            assert!(!resolved.settings.enabled);
            assert!(platform.bindings.current().is_none());
            let pool = platform.scratch.database.pool().await.unwrap();
            assert!(
                !history::load_settings::<SavedSettings>(&pool)
                    .await
                    .unwrap()
                    .settings
                    .enabled
            );
        }
        platform.scratch.database.suspend().await.unwrap();
    }
    #[tokio::test]
    async fn history_failure_does_not_block_copy() {
        for fault in [
            HistoryFault::Append,
            HistoryFault::Expire,
            HistoryFault::Unavailable,
        ] {
            resolve_with_history_failure(ResolveAction::Copy, None, fault).await;
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_block_copy_and_quit() {
        for fault in [
            HistoryFault::Append,
            HistoryFault::Expire,
            HistoryFault::Unavailable,
        ] {
            resolve_with_history_failure(ResolveAction::Copy, Some(ExitIntent::Quit), fault).await;
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_block_copy_and_disable() {
        for fault in [HistoryFault::Append, HistoryFault::Expire] {
            resolve_with_history_failure(ResolveAction::Copy, Some(ExitIntent::Disable), fault)
                .await;
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_block_confirmation() {
        for exit in [None, Some(ExitIntent::Quit), Some(ExitIntent::Disable)] {
            for fault in [
                HistoryFault::Append,
                HistoryFault::Expire,
                HistoryFault::Unavailable,
            ] {
                // An unavailable database also blocks required Disable persistence.
                if exit == Some(ExitIntent::Disable) && matches!(fault, HistoryFault::Unavailable) {
                    continue;
                }
                resolve_with_history_failure(ResolveAction::Confirm, exit, fault).await;
            }
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_report_verified_delivery_as_failed() {
        for fault in [
            HistoryFault::Append,
            HistoryFault::Expire,
            HistoryFault::Unavailable,
        ] {
            let (commands, platform) = fixture().await;
            enable(&commands, Cleanup::Raw, true).await;
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            fault.inject(&platform).await;
            let delivered = commands.stop_dictation().await.unwrap();
            assert_eq!(*platform.pasted.lock().unwrap(), vec!["raw words"]);
            assert_eq!(delivered.delivery, Delivery::Verified);
            assert_eq!(delivered.phase, Phase::Idle);
            assert!(
                delivered.error.is_none(),
                "{fault:?}: {:?}",
                delivered.error
            );
            assert!(delivered.text.is_empty());
            assert!(delivered.raw_text.is_empty());
            assert!(!platform.gate.has_dictation_review());
            assert!(platform.gate.claim(CaptureOwner::Update).is_ok());
            commands.stop_dictation().await.unwrap();
            assert!(commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .is_err());
            assert_eq!(*platform.pasted.lock().unwrap(), vec!["raw words"]);
            assert!(platform.copied.lock().unwrap().is_empty());
            fault.restore(&platform).await;
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
            // A later session can save normally; the failed optional write did
            // not poison either the database owner or lifecycle state.
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            assert!(commands.stop_dictation().await.unwrap().error.is_none());
            assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
            assert_eq!(platform.pasted.lock().unwrap().len(), 2);
            platform.scratch.database.suspend().await.unwrap();
        }
    }
    #[tokio::test]
    async fn failed_copy_keeps_review_and_pending_exit_without_saving_history() {
        for exit in [None, Some(ExitIntent::Quit), Some(ExitIntent::Disable)] {
            let (commands, platform) = completed_review(Delivery::None).await;
            request_review_exit(&commands, exit).await;
            platform.copy_error.store(true, Ordering::SeqCst);
            assert_eq!(
                commands
                    .resolve_dictation(ResolveAction::Copy)
                    .await
                    .unwrap_err(),
                "Clipboard unavailable."
            );
            let review = commands.get_dictation_state().await.unwrap();
            assert_eq!(review.phase, Phase::Review);
            assert_eq!(review.text, "raw words");
            assert_eq!(review.raw_text, "raw words");
            assert_eq!(review.exit_intent, exit);
            assert!(review.settings.enabled);
            assert!(platform.copied.lock().unwrap().is_empty());
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
            assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
            assert!(platform.gate.has_dictation_review());
            assert!(platform.gate.claim(CaptureOwner::Update).is_err());
            assert!(commands.start_dictation().await.is_err());
            platform.copy_error.store(false, Ordering::SeqCst);
            commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .unwrap();
            assert_eq!(*platform.copied.lock().unwrap(), vec!["raw words"]);
            assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
            platform.scratch.database.suspend().await.unwrap();
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_hide_failed_quit_or_release_review() {
        for action in [ResolveAction::Copy, ResolveAction::Confirm] {
            for fault in [
                HistoryFault::Append,
                HistoryFault::Expire,
                HistoryFault::Unavailable,
            ] {
                let (commands, platform) = completed_review(Delivery::Uncertain).await;
                request_review_exit(&commands, Some(ExitIntent::Quit)).await;
                let meeting = platform.gate.claim(CaptureOwner::Meeting).unwrap();
                fault.inject(&platform).await;
                let error = commands.resolve_dictation(action).await.unwrap_err();
                assert!(error.contains("Finish the current Meeting"), "{error}");
                let review = commands.get_dictation_state().await.unwrap();
                assert_eq!(review.phase, Phase::Review);
                assert_eq!(review.text, "raw words");
                assert_eq!(review.raw_text, "raw words");
                assert_eq!(review.delivery, Delivery::Uncertain);
                assert_eq!(review.error.as_deref(), Some(error.as_str()));
                assert_eq!(review.exit_intent, Some(ExitIntent::Quit));
                assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
                assert!(platform.gate.has_dictation_review());
                drop(meeting);
                assert!(platform.gate.claim(CaptureOwner::Update).is_err());
                assert!(commands.start_dictation().await.is_err());
                fault.restore(&platform).await;
                assert!(commands.list_dictation_history().await.unwrap().is_empty());
                // History can retry after a real exit failure, but only because
                // review is still pending. No automatic delivery may retry.
                platform.race_update.store(true, Ordering::SeqCst);
                commands.resolve_dictation(action).await.unwrap();
                assert_eq!(platform.quits.load(Ordering::SeqCst), 1);
                assert!(platform.update_claim.lock().unwrap().is_none());
                assert!(platform.gate.claim(CaptureOwner::Update).is_err());
                assert!(!platform.gate.has_dictation_review());
                assert_eq!(*platform.pasted.lock().unwrap(), vec!["raw words"]);
                assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
                platform.scratch.database.suspend().await.unwrap();
            }
        }
    }
    #[tokio::test]
    async fn history_failure_does_not_hide_failed_disable_or_release_review() {
        for action in [ResolveAction::Copy, ResolveAction::Confirm] {
            for fault in [HistoryFault::Append, HistoryFault::Unavailable] {
                let (commands, platform) = completed_review(Delivery::Uncertain).await;
                request_review_exit(&commands, Some(ExitIntent::Disable)).await;
                sqlx::raw_sql("CREATE TRIGGER reject_disable BEFORE UPDATE ON settings BEGIN SELECT RAISE(FAIL, 'synthetic settings storage failure'); END;")
                    .execute(&platform.pool).await.unwrap();
                fault.inject(&platform).await;
                let error = commands.resolve_dictation(action).await.unwrap_err();
                let expected = if matches!(fault, HistoryFault::Unavailable) {
                    "The app is preparing to update. Try again after it restarts."
                } else {
                    "Could not save dictation settings."
                };
                assert_eq!(error, expected);
                let review = commands.get_dictation_state().await.unwrap();
                assert_eq!(review.phase, Phase::Review);
                assert_eq!(review.text, "raw words");
                assert_eq!(review.raw_text, "raw words");
                assert_eq!(review.delivery, Delivery::Uncertain);
                assert_eq!(review.error.as_deref(), Some(expected));
                assert_eq!(review.exit_intent, Some(ExitIntent::Disable));
                assert!(review.settings.enabled);
                assert!(platform.bindings.current().is_some());
                assert!(platform.gate.has_dictation_review());
                assert!(platform.gate.claim(CaptureOwner::Update).is_err());
                assert!(platform.gate.claim(CaptureOwner::Quit).is_err());
                assert!(commands.start_dictation().await.is_err());
                fault.restore(&platform).await;
                assert!(commands.list_dictation_history().await.unwrap().is_empty());
                let pool = platform.scratch.database.pool().await.unwrap();
                assert!(
                    history::load_settings::<SavedSettings>(&pool)
                        .await
                        .unwrap()
                        .settings
                        .enabled
                );
                sqlx::raw_sql("DROP TRIGGER reject_disable;")
                    .execute(&pool)
                    .await
                    .unwrap();
                let disabled = commands.resolve_dictation(action).await.unwrap();
                assert_eq!(disabled.phase, Phase::Disabled);
                assert!(!disabled.settings.enabled);
                assert!(!platform.gate.has_dictation_review());
                assert!(platform.gate.claim(CaptureOwner::Update).is_ok());
                assert_eq!(*platform.pasted.lock().unwrap(), vec!["raw words"]);
                assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
                platform.scratch.database.suspend().await.unwrap();
            }
        }
    }
    #[tokio::test]
    async fn history_commands_reacquire_database_after_update_resume() {
        let scratch = Arc::new(ScratchDatabase::new());
        let original = scratch.database.pool().await.unwrap();
        let (commands, platform) =
            fixture_with_database(CaptureGate::default(), scratch.clone()).await;
        enable(&commands, Cleanup::Raw, true).await;
        for outcome in [Delivery::Verified, Delivery::None, Delivery::Uncertain] {
            // Updates can claim ownership only while dictation is idle.
            let update = platform.gate.claim(CaptureOwner::Update).unwrap();
            scratch.database.suspend().await.unwrap();
            assert!(original.is_closed());
            assert!(commands.list_dictation_history().await.is_err());
            assert!(commands.delete_dictation_history(None).await.is_err());
            assert!(commands.start_dictation().await.is_err());
            scratch.database.resume().await;
            drop(update);

            *platform.outcome.lock().unwrap() = outcome;
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            let result = commands.stop_dictation().await.unwrap();
            assert!(
                result.error.is_none(),
                "history after resume: {:?}",
                result.error
            );
            if outcome != Delivery::Verified {
                assert_eq!(result.phase, Phase::Review);
                commands
                    .resolve_dictation(if outcome == Delivery::Uncertain {
                        ResolveAction::Confirm
                    } else {
                        ResolveAction::Copy
                    })
                    .await
                    .expect("review history must use the resumed database");
            }
            let rows = commands.list_dictation_history().await.unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].text, "raw words");
            commands
                .delete_dictation_history(Some(rows[0].id))
                .await
                .unwrap();
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
        }
        // Seed rows directly so append does not perform expiry for the timer.
        let pool = scratch.database.pool().await.unwrap();
        for (text, age) in [("expired", 31), ("retained", 1)] {
            sqlx::query("INSERT INTO dictation_history(text, created_at) VALUES (?, ?)")
                .bind(text)
                .bind((chrono::Utc::now() - chrono::Duration::days(age)).to_rfc3339())
                .execute(&pool)
                .await
                .unwrap();
        }
        scratch.database.suspend().await.unwrap();
        assert!(commands.expire_history().await.is_err());
        scratch.database.resume().await;
        // Exercise the same operation used by the resident expiry timer, then
        // read SQLite before list can expire anything on the command's behalf.
        commands.expire_history().await.unwrap();
        let pool = scratch.database.pool().await.unwrap();
        let rows: Vec<String> = sqlx::query_scalar("SELECT text FROM dictation_history")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(rows, ["retained"]);
        assert_eq!(
            commands.list_dictation_history().await.unwrap()[0].text,
            "retained"
        );
        scratch.database.suspend().await.unwrap();
        scratch.database.resume().await;
        commands.delete_dictation_history(None).await.unwrap();
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        scratch.database.suspend().await.unwrap();
    }
    #[tokio::test]
    async fn settings_commands_reacquire_database_after_update_resume() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, false).await;
        let database = &platform.scratch.database;
        let original = database.pool().await.unwrap();
        let before = commands.get_dictation_state().await.unwrap().settings;
        let mut input = SettingsInput {
            settings: before.clone(),
            consent_provider: false,
        };
        input.settings.shortcut = "Ctrl+Alt+D".into();
        input.settings.side = Side::Left;
        input.settings.history_enabled = false;
        for _ in 0..2 {
            let update = platform.gate.claim(CaptureOwner::Update).unwrap();
            database.suspend().await.unwrap();
            assert!(original.is_closed());
            let active = commands.get_dictation_state().await.unwrap().settings;
            let binding = platform.bindings.current();
            assert!(commands
                .set_dictation_settings(input.clone())
                .await
                .is_err());
            assert_eq!(
                commands.get_dictation_state().await.unwrap().settings,
                active
            );
            assert_eq!(platform.bindings.current(), binding);
            database.resume().await;
            drop(update);
            let state = commands
                .set_dictation_settings(input.clone())
                .await
                .expect("settings must use the resumed database");
            assert_eq!(state.settings, input.settings);
            assert_eq!(
                platform.bindings.current(),
                Some(input.settings.shortcut.clone())
            );
            let reopened =
                DictationCommands::new(database.clone(), platform.clone(), CaptureGate::default())
                    .await
                    .unwrap();
            assert_eq!(
                reopened.get_dictation_state().await.unwrap().settings,
                input.settings
            );
            input.settings = before.clone();
        }
        database.suspend().await.unwrap();
    }
    #[tokio::test]
    async fn provider_adapter_reloads_consent_and_cleanup_config_after_update_resume() {
        let (commands, platform) = fixture().await;
        let database = &platform.scratch.database;
        let original = database.pool().await.unwrap();
        let config = serde_json::json!({
            "kind": "open_ai",
            "base_url": "https://dictation.example.invalid/initial/v1",
            "model": "synthetic-model",
        });
        sqlx::query("INSERT INTO settings(key, value) VALUES ('llm_config', ?)")
            .bind(config.to_string())
            .execute(&original)
            .await
            .unwrap();
        platform
            .use_configured_provider
            .store(true, Ordering::SeqCst);
        enable(&commands, Cleanup::Provider, false).await;
        assert!(
            commands
                .get_dictation_state()
                .await
                .unwrap()
                .provider_authorized
        );
        for endpoint in ["changed", "changed-again"] {
            let update = platform.gate.claim(CaptureOwner::Update).unwrap();
            database.suspend().await.unwrap();
            assert!(original.is_closed());
            assert!(
                !commands
                    .get_dictation_state()
                    .await
                    .unwrap()
                    .provider_authorized
            );
            database.resume().await;
            drop(update);
            assert!(
                commands
                    .get_dictation_state()
                    .await
                    .unwrap()
                    .provider_authorized,
                "saved provider consent must recover after database resume"
            );

            // Revoke consent by changing the persisted destination. The real
            // cleanup adapter must reject before credential lookup or HTTP.
            let pool = database.pool().await.unwrap();
            let mut changed = config.clone();
            changed["base_url"] = format!("https://dictation.example.invalid/{endpoint}/v1").into();
            sqlx::query("UPDATE settings SET value = ? WHERE key = 'llm_config'")
                .bind(changed.to_string())
                .execute(&pool)
                .await
                .unwrap();
            assert!(
                !commands
                    .get_dictation_state()
                    .await
                    .unwrap()
                    .provider_authorized
            );
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            let review = commands.stop_dictation().await.unwrap();
            assert_eq!(review.phase, Phase::Review);
            assert_eq!(review.text, "raw words");
            assert_eq!(
                review.error.as_deref(),
                Some("Dictation provider changed; renew cleanup consent")
            );
            assert!(platform.pasted.lock().unwrap().is_empty());
            commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .unwrap();
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
            let renewed = commands
                .set_dictation_settings(SettingsInput {
                    settings: review.settings,
                    consent_provider: true,
                })
                .await
                .unwrap();
            assert!(renewed.provider_authorized);
        }
        database.suspend().await.unwrap();
    }
    #[tokio::test]
    async fn completed_raw_dictation_delivers_once_and_history_can_be_deleted() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, true).await;
        assert_eq!(
            commands.start_dictation().await.unwrap().phase,
            Phase::Starting
        );
        wait_phase(&commands, Phase::Listening).await;
        assert!(commands.start_dictation().await.is_err());
        let result = commands.stop_dictation().await.unwrap();
        assert_eq!(result.phase, Phase::Idle);
        assert_eq!(*platform.pasted.lock().unwrap(), vec!["raw words"]);
        let rows = commands.list_dictation_history().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "raw words");
        commands.delete_dictation_history(None).await.unwrap();
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn ten_minute_limit_retains_text_without_delivery_or_history() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, true).await;
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(601)).await;
        tokio::task::yield_now().await;
        tokio::time::resume();
        wait_phase(&commands, Phase::Review).await;
        assert!(platform.pasted.lock().unwrap().is_empty());
        commands
            .resolve_dictation(ResolveAction::Copy)
            .await
            .unwrap();
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
    }
    async fn wait_cleanup(platform: &FakePlatform) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !platform.cleanup_started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn cancellation_during_cleanup_discards_and_late_events_cannot_change_new_session() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Provider, true).await;
        platform.slow_cleanup.store(true, Ordering::SeqCst);
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        let stale = platform.sink.lock().unwrap().clone().unwrap();
        let stop = tokio::spawn({
            let commands = commands.clone();
            async move { commands.stop_dictation().await }
        });
        wait_cleanup(&platform).await;
        let cancelled = commands.cancel_dictation().await.unwrap();
        assert_eq!(cancelled.phase, Phase::Idle);
        assert!(cancelled.text.is_empty());
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        stop.await.unwrap().unwrap();
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        stale(AudioEvent::Text("stale private text".into()));
        stale(AudioEvent::Failed("stale failure".into()));
        platform.cleanup_release.notify_one();
        tokio::task::yield_now().await;
        let current = commands.get_dictation_state().await.unwrap();
        assert_eq!(current.text, "raw words");
        assert_eq!(current.phase, Phase::Listening);
        assert!(platform.pasted.lock().unwrap().is_empty());
        commands.cancel_dictation().await.unwrap();
    }
    #[tokio::test]
    async fn blocked_quit_keeps_review_and_can_be_cancelled_without_resuming() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, false).await;
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        commands.stop_dictation().await.unwrap();
        commands.request_quit().await.unwrap();
        let meeting = platform.gate.claim(CaptureOwner::Meeting).unwrap();
        assert!(commands
            .resolve_dictation(ResolveAction::Copy)
            .await
            .is_err());
        let state = commands.get_dictation_state().await.unwrap();
        assert_eq!(state.phase, Phase::Review);
        assert_eq!(state.text, "raw words");
        assert!(state.error.unwrap().contains("Meeting"));
        commands
            .resolve_dictation(ResolveAction::CancelExit)
            .await
            .unwrap();
        assert_eq!(
            commands.get_dictation_state().await.unwrap().exit_intent,
            None
        );
        assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
        assert!(platform.gate.has_dictation_review());
        assert!(platform.gate.claim(CaptureOwner::Update).is_err());
        drop(meeting);
        assert!(platform.gate.has_dictation_review());
        assert!(platform.gate.claim(CaptureOwner::Update).is_err());
        commands
            .resolve_dictation(ResolveAction::Copy)
            .await
            .unwrap();
        assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn quit_and_disable_present_review_without_capturing_again() {
        for disable in [false, true] {
            for locked in [false, true] {
                let (commands, platform) = fixture().await;
                enable(&commands, Cleanup::Provider, true).await;
                commands.start_dictation().await.unwrap();
                wait_phase(&commands, Phase::Listening).await;
                platform.desktop.store(!locked, Ordering::SeqCst);
                if disable {
                    let mut settings = commands.get_dictation_state().await.unwrap().settings;
                    settings.enabled = false;
                    commands
                        .set_dictation_settings(SettingsInput {
                            settings,
                            consent_provider: false,
                        })
                        .await
                        .unwrap();
                } else {
                    commands.request_quit().await.unwrap();
                }
                let state = commands.get_dictation_state().await.unwrap();
                assert_eq!(state.phase, Phase::Review);
                assert_eq!(
                    state.exit_intent,
                    Some(if disable {
                        ExitIntent::Disable
                    } else {
                        ExitIntent::Quit
                    })
                );
                assert_eq!(
                    platform.review_shown.load(Ordering::SeqCst),
                    usize::from(!locked)
                );
                assert_eq!(platform.capture_starts.load(Ordering::SeqCst), 1);
                assert!(platform.capture_finished.load(Ordering::SeqCst));
                assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
                assert!(!platform.cleanup_started.load(Ordering::SeqCst));
                assert!(platform.pasted.lock().unwrap().is_empty());
                commands
                    .resolve_dictation(ResolveAction::CancelExit)
                    .await
                    .unwrap();
                commands
                    .resolve_dictation(ResolveAction::Dismiss)
                    .await
                    .unwrap();
            }
        }
    }
    #[tokio::test]
    async fn resolving_quit_review_never_opens_an_update_claim_gap() {
        for action in [
            Some(ResolveAction::Copy),
            Some(ResolveAction::Dismiss),
            Some(ResolveAction::Confirm),
            None,
        ] {
            let gate = CaptureGate::default();
            let (commands, platform) = fixture_with_gate(gate.clone()).await;
            *platform.outcome.lock().unwrap() = Delivery::Uncertain;
            enable(&commands, Cleanup::Raw, true).await;
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            commands.stop_dictation().await.unwrap();
            commands.request_quit().await.unwrap();
            assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
            assert!(gate.claim(CaptureOwner::Quit).is_err());
            platform.race_update.store(true, Ordering::SeqCst);
            let result = if let Some(action) = action {
                commands.resolve_dictation(action).await
            } else {
                commands.cancel_dictation().await
            };
            result.expect(
                "resolved review must transfer atomically to Quit, without admitting Update",
            );
            assert!(platform.update_claim.lock().unwrap().is_none());
            assert_eq!(platform.quits.load(Ordering::SeqCst), 1);
            assert!(!gate.has_dictation_review());
            assert!(gate.claim(CaptureOwner::Meeting).is_err());
            assert!(gate.claim(CaptureOwner::Dictation).is_err());
            assert!(gate.claim(CaptureOwner::Update).is_err());
        }
    }
    #[tokio::test]
    async fn ordinary_quit_cannot_consume_unresolved_gate_review() {
        let gate = CaptureGate::default();
        let (commands, platform) = fixture_with_gate(gate.clone()).await;
        gate.set_dictation_review(true);
        assert!(commands.request_quit().await.is_err());
        assert_eq!(platform.quits.load(Ordering::SeqCst), 0);
        assert!(gate.has_dictation_review());
        assert!(gate.claim(CaptureOwner::Update).is_err());
        assert!(gate.claim(CaptureOwner::Meeting).is_ok());
    }
    #[tokio::test]
    async fn uncertain_delivery_is_never_retried_and_only_explicit_confirmation_saves_it() {
        let (commands, platform) = fixture().await;
        *platform.outcome.lock().unwrap() = Delivery::Uncertain;
        enable(&commands, Cleanup::Provider, true).await;
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        let review = commands.stop_dictation().await.unwrap();
        assert_eq!(review.phase, Phase::Review);
        assert_eq!(review.text, "Clean words.");
        assert_eq!(review.raw_text, "raw words");
        assert_eq!(review.delivery, Delivery::Uncertain);
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        commands.stop_dictation().await.unwrap();
        assert!(commands.start_dictation().await.is_err());
        commands
            .resolve_dictation(ResolveAction::Confirm)
            .await
            .unwrap();
        assert_eq!(*platform.pasted.lock().unwrap(), vec!["Clean words."]);
        assert_eq!(
            commands.list_dictation_history().await.unwrap()[0].text,
            "Clean words."
        );
    }
    #[tokio::test]
    async fn cleanup_failure_and_changed_provider_consent_retain_raw_only_in_memory() {
        for change_identity in [false, true] {
            let (commands, platform) = fixture().await;
            enable(&commands, Cleanup::Provider, true).await;
            if change_identity {
                *platform.identity.lock().unwrap() = "openai|https://two.test/other-path".into();
                assert!(
                    !commands
                        .get_dictation_state()
                        .await
                        .unwrap()
                        .provider_authorized
                );
            } else {
                platform.cleanup_error.store(true, Ordering::SeqCst);
            }
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            let review = commands.stop_dictation().await.unwrap();
            assert_eq!(review.phase, Phase::Review);
            assert_eq!(review.text, "raw words");
            assert!(review.error.is_some());
            assert!(platform.pasted.lock().unwrap().is_empty());
            assert!(commands
                .resolve_dictation(ResolveAction::Confirm)
                .await
                .is_err());
            let reopened = DictationCommands::new(
                platform.scratch.database.clone(),
                platform.clone(),
                CaptureGate::default(),
            )
            .await
            .unwrap();
            assert!(reopened
                .get_dictation_state()
                .await
                .unwrap()
                .text
                .is_empty());
            assert!(reopened.list_dictation_history().await.unwrap().is_empty());
            commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .unwrap();
            assert_eq!(*platform.copied.lock().unwrap(), vec!["raw words"]);
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn disabling_during_cleanup_cancels_upload_and_requires_copy_or_discard() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Provider, true).await;
        platform.slow_cleanup.store(true, Ordering::SeqCst);
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        let stop = tokio::spawn({
            let commands = commands.clone();
            async move { commands.stop_dictation().await }
        });
        wait_cleanup(&platform).await;
        let mut settings = commands.get_dictation_state().await.unwrap().settings;
        settings.enabled = false;
        let review = commands
            .set_dictation_settings(SettingsInput {
                settings,
                consent_provider: false,
            })
            .await
            .unwrap();
        assert_eq!(review.phase, Phase::Review);
        assert_eq!(review.exit_intent, Some(ExitIntent::Disable));
        assert!(review.settings.enabled);
        platform.cleanup_release.notify_one();
        stop.await.unwrap().unwrap();
        let disabled = commands
            .resolve_dictation(ResolveAction::Copy)
            .await
            .unwrap();
        assert_eq!(disabled.phase, Phase::Disabled);
        assert!(!disabled.settings.enabled);
        assert!(commands.list_dictation_history().await.unwrap().is_empty());
        assert!(platform.pasted.lock().unwrap().is_empty());
        let reopened = DictationCommands::new(
            platform.scratch.database.clone(),
            platform.clone(),
            CaptureGate::default(),
        )
        .await
        .unwrap();
        assert!(
            !reopened
                .get_dictation_state()
                .await
                .unwrap()
                .settings
                .enabled
        );
    }
    #[tokio::test]
    async fn failed_rebinding_keeps_active_and_persisted_settings_and_retries_reserved_cleanup() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, false).await;
        let before = commands.get_dictation_state().await.unwrap().settings;
        let mut after = before.clone();
        after.shortcut = "Ctrl+Alt+D".into();
        platform
            .bindings
            .fail_unregister
            .store(true, Ordering::SeqCst);
        let error = commands
            .set_dictation_settings(SettingsInput {
                settings: after.clone(),
                consent_provider: false,
            })
            .await
            .unwrap_err();
        assert!(error.contains("reserved but inactive"));
        assert_eq!(
            commands.get_dictation_state().await.unwrap().settings,
            before
        );
        let reopened = DictationCommands::new(
            platform.scratch.database.clone(),
            platform.clone(),
            CaptureGate::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            reopened.get_dictation_state().await.unwrap().settings,
            before
        );
        platform
            .bindings
            .fail_unregister
            .store(false, Ordering::SeqCst);
        commands
            .set_dictation_settings(SettingsInput {
                settings: after.clone(),
                consent_provider: false,
            })
            .await
            .unwrap();
        assert_eq!(
            commands.get_dictation_state().await.unwrap().settings,
            after
        );
    }
    #[tokio::test]
    async fn shortcut_conflict_alias_and_failed_sqlite_save_leave_previous_binding_working() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, false).await;
        let mut settings = commands.get_dictation_state().await.unwrap().settings;
        // Parsed-equivalent names must not attempt a duplicate OS registration.
        settings.shortcut = "Control+Shift+Space".into();
        commands
            .set_dictation_settings(SettingsInput {
                settings: settings.clone(),
                consent_provider: false,
            })
            .await
            .unwrap();
        *platform.bindings.meeting.lock().unwrap() = Some("Ctrl+Alt+M".into());
        let mut conflicting = settings.clone();
        conflicting.shortcut = "Control+Alt+M".into();
        assert!(commands
            .set_dictation_settings(SettingsInput {
                settings: conflicting,
                consent_provider: false
            })
            .await
            .unwrap_err()
            .contains("Meeting"));
        sqlx::raw_sql("CREATE TRIGGER reject_dictation_update BEFORE UPDATE ON settings BEGIN SELECT RAISE(FAIL, 'synthetic disk failure'); END;").execute(&platform.pool).await.unwrap();
        let mut replacement = settings.clone();
        replacement.shortcut = "Ctrl+Alt+D".into();
        assert!(commands
            .set_dictation_settings(SettingsInput {
                settings: replacement.clone(),
                consent_provider: false
            })
            .await
            .is_err());
        assert_eq!(
            commands.get_dictation_state().await.unwrap().settings,
            settings
        );
        sqlx::raw_sql("DROP TRIGGER reject_dictation_update;")
            .execute(&platform.pool)
            .await
            .unwrap();
        commands
            .set_dictation_settings(SettingsInput {
                settings: replacement,
                consent_provider: false,
            })
            .await
            .unwrap();
        commands.start_dictation().await.unwrap();
        commands.cancel_dictation().await.unwrap();
    }
    #[tokio::test]
    async fn hold_activation_stops_when_any_chord_key_is_released() {
        let (commands, platform) = fixture().await;
        let settings = DictationSettings {
            enabled: true,
            activation: Activation::Hold,
            ..Default::default()
        };
        commands
            .set_dictation_settings(SettingsInput {
                settings,
                consent_provider: false,
            })
            .await
            .unwrap();
        commands.activate_shortcut().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        // The OS boundary reports false when a modifier is released even if
        // the primary key is still down. No Released plugin event is sent.
        platform.held.store(false, Ordering::SeqCst);
        wait_phase(&commands, Phase::Review).await;
        assert_eq!(platform.capture_starts.load(Ordering::SeqCst), 1);
        assert!(platform.pasted.lock().unwrap().is_empty());
        commands
            .resolve_dictation(ResolveAction::Dismiss)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn real_ownership_gate_excludes_active_work_but_meeting_can_run_during_review() {
        let gate = CaptureGate::default();
        let (commands, platform) = fixture_with_gate(gate.clone()).await;
        enable(&commands, Cleanup::Raw, false).await;
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        assert!(commands.start_dictation().await.is_err());
        assert_eq!(platform.capture_starts.load(Ordering::SeqCst), 0);
        drop(meeting);
        commands.start_dictation().await.unwrap();
        assert!(gate.claim(CaptureOwner::Meeting).is_err());
        assert!(gate.claim(CaptureOwner::Update).is_err());
        wait_phase(&commands, Phase::Listening).await;
        commands.stop_dictation().await.unwrap();
        assert!(gate.claim(CaptureOwner::Update).is_err());
        assert!(commands.start_dictation().await.is_err());
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        commands
            .resolve_dictation(ResolveAction::Dismiss)
            .await
            .unwrap();
        assert!(commands.start_dictation().await.is_err());
        drop(meeting);
        assert!(gate.claim(CaptureOwner::Update).is_ok());
    }
    #[tokio::test]
    async fn cancellation_waits_for_startup_and_finalization_before_releasing_ownership() {
        for phase in [Phase::Processing, Phase::Starting, Phase::Listening] {
            let during_start = phase == Phase::Starting;
            let gate = CaptureGate::default();
            let (commands, platform) = fixture_with_gate(gate.clone()).await;
            enable(&commands, Cleanup::Provider, true).await;
            platform.slow_start.store(during_start, Ordering::SeqCst);
            platform.slow_finish.store(true, Ordering::SeqCst);
            commands.start_dictation().await.unwrap();
            let stop = if during_start {
                wait_until(|| platform.capture_starts.load(Ordering::SeqCst) != 0).await;
                None
            } else {
                wait_phase(&commands, Phase::Listening).await;
                if phase == Phase::Processing {
                    let stop = tokio::spawn({
                        let commands = commands.clone();
                        async move { commands.stop_dictation().await }
                    });
                    wait_until(|| platform.finish_started.load(Ordering::SeqCst)).await;
                    Some(stop)
                } else {
                    None
                }
            };
            let mut cancel = Box::pin(commands.cancel_dictation());
            assert!(
                tokio::time::timeout(Duration::from_millis(30), &mut cancel)
                    .await
                    .is_err(),
                "cancel must await native teardown, not just request it"
            );
            assert!(gate.claim(CaptureOwner::Meeting).is_err());
            assert!(gate.claim(CaptureOwner::Update).is_err());
            assert!(!platform.capture_finished.load(Ordering::SeqCst));
            platform.start_release.notify_one();
            assert!(tokio::time::timeout(Duration::from_millis(30), &mut cancel)
                .await
                .is_err());
            assert!(platform.finish_started.load(Ordering::SeqCst));
            assert!(platform.capture_stopped.load(Ordering::SeqCst));
            assert!(gate.claim(CaptureOwner::Meeting).is_err());
            platform.finish_release.notify_one();
            let state = tokio::time::timeout(Duration::from_secs(3), cancel)
                .await
                .unwrap()
                .unwrap();
            if let Some(stop) = stop {
                stop.await.unwrap().unwrap();
            }
            assert_eq!(state.phase, Phase::Idle);
            assert!(state.text.is_empty());
            assert!(state.raw_text.is_empty());
            assert!(platform.pasted.lock().unwrap().is_empty());
            assert!(!platform.cleanup_started.load(Ordering::SeqCst));
            assert!(platform.capture_finished.load(Ordering::SeqCst));
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
            assert!(gate.claim(CaptureOwner::Meeting).is_ok());
            assert!(gate.claim(CaptureOwner::Update).is_ok());
            if during_start {
                assert!(!platform
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|state| state.phase == Phase::Listening));
            }
        }
    }
    #[tokio::test]
    async fn cancellation_during_model_preparation_never_opens_a_microphone_later() {
        let gate = CaptureGate::default();
        let (commands, platform) = fixture_with_gate(gate.clone()).await;
        enable(&commands, Cleanup::Provider, true).await;
        platform.slow_prepare.store(true, Ordering::SeqCst);
        commands.start_dictation().await.unwrap();
        wait_until(|| platform.preparing.load(Ordering::SeqCst)).await;
        let state = tokio::time::timeout(Duration::from_secs(1), commands.cancel_dictation())
            .await
            .expect("cancellation must not wait for model preparation")
            .unwrap();
        platform.prepare_release.notify_one();
        tokio::task::yield_now().await;
        assert_eq!(state.phase, Phase::Idle);
        assert_eq!(platform.capture_starts.load(Ordering::SeqCst), 0);
        assert!(gate.claim(CaptureOwner::Meeting).is_ok());
        assert!(!platform.cleanup_started.load(Ordering::SeqCst));
        assert!(platform.pasted.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn interruption_drains_late_recovery_text_before_releasing_capture_ownership() {
        for phase in [Phase::Starting, Phase::Listening, Phase::Processing] {
            let gate = CaptureGate::default();
            let (commands, platform) = fixture_with_gate(gate.clone()).await;
            enable(&commands, Cleanup::Provider, true).await;
            platform
                .slow_start
                .store(phase == Phase::Starting, Ordering::SeqCst);
            platform.slow_finish.store(true, Ordering::SeqCst);
            platform.finish_error.store(true, Ordering::SeqCst);
            *platform.finish_text.lock().unwrap() = "late drained recovery".into();
            commands.start_dictation().await.unwrap();
            let stop = if phase == Phase::Starting {
                wait_until(|| platform.capture_starts.load(Ordering::SeqCst) != 0).await;
                None
            } else {
                wait_phase(&commands, Phase::Listening).await;
                if phase == Phase::Processing {
                    let stopping = commands.clone();
                    let stop = tokio::spawn(async move { stopping.stop_dictation().await });
                    wait_phase(&commands, Phase::Processing).await;
                    Some(stop)
                } else {
                    None
                }
            };
            if phase == Phase::Starting {
                commands.interrupt("Windows locked during startup.");
                platform.start_release.notify_one();
            } else {
                // Native Failed can precede the decoder's final Text callback.
                platform.sink.lock().unwrap().clone().unwrap()(AudioEvent::Failed(
                    "Device lost.".into(),
                ));
            }
            wait_phase(&commands, Phase::Processing).await;
            assert!(gate.claim(CaptureOwner::Meeting).is_err());
            assert!(gate.claim(CaptureOwner::Update).is_err());
            assert!(commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .is_err());
            assert!(!platform.capture_finished.load(Ordering::SeqCst));
            platform.finish_release.notify_one();
            let review = commands.stop_dictation().await.unwrap();
            if let Some(stop) = stop {
                stop.await.unwrap().unwrap();
            }
            assert_eq!(review.phase, Phase::Review);
            assert_eq!(review.text, "late drained recovery");
            assert_eq!(review.raw_text, "late drained recovery");
            assert!(review.error.is_some());
            assert!(platform.capture_finished.load(Ordering::SeqCst));
            assert!(gate.has_dictation_review());
            assert!(gate.claim(CaptureOwner::Update).is_err());
            let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
            assert!(commands.start_dictation().await.is_err());
            commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .unwrap();
            assert_eq!(
                *platform.copied.lock().unwrap(),
                vec!["late drained recovery"]
            );
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
            assert!(!platform.cleanup_started.load(Ordering::SeqCst));
            assert!(platform.pasted.lock().unwrap().is_empty());
            assert!(gate.claim(CaptureOwner::Update).is_err());
            drop(meeting);
            assert!(gate.claim(CaptureOwner::Update).is_ok());
            if phase == Phase::Starting {
                assert!(!platform
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|state| state.phase == Phase::Listening));
            }
        }
    }
    #[tokio::test]
    async fn lock_and_device_failure_stop_without_delivery_and_keep_failed_text_out_of_history() {
        for device_loss in [true, false] {
            let (commands, platform) = fixture().await;
            enable(&commands, Cleanup::Raw, true).await;
            commands.start_dictation().await.unwrap();
            wait_phase(&commands, Phase::Listening).await;
            let sink = platform.sink.lock().unwrap().clone().unwrap();
            sink(AudioEvent::Level(f32::NAN));
            assert_eq!(commands.get_dictation_state().await.unwrap().level, 0.0);
            sink(AudioEvent::Level(3.0));
            assert_eq!(commands.get_dictation_state().await.unwrap().level, 1.0);
            if device_loss {
                sink(AudioEvent::Failed("Microphone disconnected.".into()));
            } else {
                platform.desktop.store(false, Ordering::SeqCst);
            }
            wait_phase(&commands, Phase::Review).await;
            assert!(platform.capture_stopped.load(Ordering::SeqCst));
            assert!(platform.pasted.lock().unwrap().is_empty());
            platform.desktop.store(true, Ordering::SeqCst);
            commands
                .resolve_dictation(ResolveAction::Copy)
                .await
                .unwrap();
            assert!(commands.list_dictation_history().await.unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn settings_freeze_microphone_and_history_opt_out_keeps_existing_entries() {
        let (commands, platform) = fixture().await;
        enable(&commands, Cleanup::Raw, true).await;
        let mut settings = commands.get_dictation_state().await.unwrap().settings;
        settings.microphone_id = Some("device A".into());
        commands
            .set_dictation_settings(SettingsInput {
                settings: settings.clone(),
                consent_provider: false,
            })
            .await
            .unwrap();
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        settings.microphone_id = Some("device B".into());
        assert!(commands
            .set_dictation_settings(SettingsInput {
                settings: settings.clone(),
                consent_provider: false
            })
            .await
            .is_err());
        assert_eq!(
            *platform.microphone.lock().unwrap(),
            Some("device A".into())
        );
        commands.stop_dictation().await.unwrap();
        settings.history_enabled = false;
        commands
            .set_dictation_settings(SettingsInput {
                settings,
                consent_provider: false,
            })
            .await
            .unwrap();
        commands.start_dictation().await.unwrap();
        wait_phase(&commands, Phase::Listening).await;
        assert_eq!(
            *platform.microphone.lock().unwrap(),
            Some("device B".into())
        );
        commands.stop_dictation().await.unwrap();
        assert_eq!(commands.list_dictation_history().await.unwrap().len(), 1);
    }
    #[tokio::test]
    async fn disabled_command_cannot_open_the_microphone() {
        let (commands, platform) = fixture().await;
        assert!(commands.start_dictation().await.is_err());
        assert_eq!(platform.capture_starts.load(Ordering::SeqCst), 0);
        assert_eq!(
            commands.get_dictation_state().await.unwrap().phase,
            Phase::Disabled
        );
    }
}
