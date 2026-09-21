use super::{replace, ShortcutBackend};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};

#[derive(Clone, Copy)]
enum SaveOutcome {
    Success,
    Fail,
    CommitThenFail,
}

struct MemoryData {
    registered: BTreeSet<String>,
    saved: Option<String>,
    calls: Vec<String>,
    unavailable: BTreeSet<String>,
    cannot_unregister: BTreeSet<String>,
    saves: VecDeque<SaveOutcome>,
    read_fails: bool,
}

struct MemoryBackend {
    data: Mutex<MemoryData>,
    pause_save: AtomicBool,
    resume: Notify,
}

#[async_trait::async_trait]
impl ShortcutBackend for MemoryBackend {
    type Shortcut = String;

    fn parse(&self, value: &str) -> Result<String, String> {
        match value {
            "invalid" => Err("invalid shortcut".into()),
            "alias-old" => Ok("old".into()),
            value => Ok(value.into()),
        }
    }

    fn register(&self, shortcut: &String) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.calls.push(format!("register:{shortcut}"));
        if data.unavailable.contains(shortcut) || !data.registered.insert(shortcut.clone()) {
            return Err("shortcut unavailable".into());
        }
        Ok(())
    }

    fn unregister(&self, shortcut: &String) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.calls.push(format!("unregister:{shortcut}"));
        if data.cannot_unregister.contains(shortcut) {
            return Err("unregistration refused".into());
        }
        assert!(data.registered.remove(shortcut), "unregistering an unowned shortcut");
        Ok(())
    }

    async fn load(&self) -> Result<Option<String>, String> {
        let data = self.data.lock().unwrap();
        if data.read_fails {
            Err("read failed".into())
        } else {
            Ok(data.saved.clone())
        }
    }

    async fn save(&self, shortcut: Option<&str>) -> Result<(), String> {
        if self.pause_save.swap(false, Ordering::SeqCst) {
            self.resume.notified().await;
        }
        let mut data = self.data.lock().unwrap();
        data.calls.push(format!("save:{}", shortcut.unwrap_or("<none>")));
        match data.saves.pop_front().unwrap_or(SaveOutcome::Success) {
            SaveOutcome::Success => {
                data.saved = shortcut.map(str::to_owned);
                Ok(())
            }
            SaveOutcome::Fail => Err("write failed".into()),
            SaveOutcome::CommitThenFail => {
                data.saved = shortcut.map(str::to_owned);
                Err("commit acknowledgement lost".into())
            }
        }
    }
}

struct Fixture {
    current: Mutex<Option<String>>,
    changes: AsyncMutex<Vec<String>>,
    backend: MemoryBackend,
}

impl Fixture {
    fn new(old: Option<&str>) -> Self {
        Self {
            current: Mutex::new(old.map(str::to_owned)),
            changes: AsyncMutex::new(Vec::new()),
            backend: MemoryBackend {
                data: Mutex::new(MemoryData {
                    registered: old.into_iter().map(str::to_owned).collect(),
                    saved: old.map(str::to_owned),
                    calls: Vec::new(),
                    unavailable: BTreeSet::new(),
                    cannot_unregister: BTreeSet::new(),
                    saves: VecDeque::new(),
                    read_fails: false,
                }),
                pause_save: AtomicBool::new(false),
                resume: Notify::new(),
            },
        }
    }

    async fn set(&self, value: Option<&str>) -> Result<(), String> {
        replace(&self.current, &self.changes, &self.backend, value.map(str::to_owned)).await
    }

    fn assert_state(&self, expected: Option<&str>) {
        assert_eq!(self.current.lock().unwrap().as_deref(), expected);
        let data = self.backend.data.lock().unwrap();
        assert_eq!(data.saved.as_deref(), expected);
        assert_eq!(data.registered, expected.into_iter().map(str::to_owned).collect());
    }
}

#[tokio::test]
async fn invalid_replacement_never_touches_the_working_shortcut() {
    let fixture = Fixture::new(Some("old"));
    assert!(fixture.set(Some("invalid")).await.unwrap_err().contains("invalid"));
    fixture.assert_state(Some("old"));
    assert!(fixture.backend.data.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn unavailable_replacement_preserves_the_working_shortcut() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().unavailable.insert("new".into());
    assert!(fixture.set(Some("new")).await.unwrap_err().contains("unavailable"));
    fixture.assert_state(Some("old"));
    assert_eq!(fixture.backend.data.lock().unwrap().calls, ["register:new"]);
}

#[tokio::test]
async fn successful_replacement_keeps_old_registered_until_the_new_value_is_saved() {
    let fixture = Fixture::new(Some("old"));
    fixture.set(Some("new")).await.unwrap();
    fixture.assert_state(Some("new"));
    assert_eq!(fixture.backend.data.lock().unwrap().calls,
        ["register:new", "save:new", "unregister:old"]);
}

#[tokio::test]
async fn failed_persistence_removes_only_the_candidate() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().saves.push_back(SaveOutcome::Fail);
    assert!(fixture.set(Some("new")).await.unwrap_err().contains("write failed"));
    fixture.assert_state(Some("old"));
    let data = fixture.backend.data.lock().unwrap();
    assert!(data.calls.contains(&"unregister:new".into()));
    assert!(!data.calls.contains(&"unregister:old".into()));
}

#[tokio::test]
async fn uncertain_commit_is_compensated_using_the_actual_saved_snapshot() {
    let fixture = Fixture::new(Some("old"));
    {
        let mut data = fixture.backend.data.lock().unwrap();
        // Startup can leave a saved value different from the active registration.
        data.saved = Some("saved-before".into());
        data.saves.push_back(SaveOutcome::CommitThenFail);
    }
    assert!(fixture.set(Some("new")).await.is_err());
    assert_eq!(fixture.current.lock().unwrap().as_deref(), Some("old"));
    let data = fixture.backend.data.lock().unwrap();
    assert_eq!(data.saved.as_deref(), Some("saved-before"));
    assert_eq!(data.registered, BTreeSet::from(["old".into()]));
}

#[tokio::test]
async fn failed_candidate_cleanup_is_reported_and_retried_before_the_next_change() {
    let fixture = Fixture::new(Some("old"));
    {
        let mut data = fixture.backend.data.lock().unwrap();
        data.saves.push_back(SaveOutcome::Fail);
        data.cannot_unregister.insert("new".into());
    }
    let error = fixture.set(Some("new")).await.unwrap_err();
    assert!(error.contains("write failed"));
    assert!(error.contains("rollback failed"));
    assert!(error.contains("unregistration refused"));
    assert_eq!(fixture.current.lock().unwrap().as_deref(), Some("old"));
    assert_eq!(fixture.backend.data.lock().unwrap().registered,
        BTreeSet::from(["old".into(), "new".into()]));
    assert_eq!(*fixture.changes.lock().await, ["new"]);

    // While cleanup still fails, a further candidate must not get registered.
    assert!(fixture.set(Some("next")).await.unwrap_err().contains("cleanup"));
    assert!(!fixture.backend.data.lock().unwrap().registered.contains("next"));
    fixture.backend.data.lock().unwrap().cannot_unregister.clear();
    fixture.set(Some("new")).await.unwrap();
    fixture.assert_state(Some("new"));
    assert!(fixture.changes.lock().await.is_empty());
}

#[tokio::test]
async fn failed_old_unregistration_rolls_back_the_saved_value_and_candidate() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().cannot_unregister.insert("old".into());
    assert!(fixture.set(Some("new")).await.unwrap_err().contains("unregistration refused"));
    fixture.assert_state(Some("old"));
}

#[tokio::test]
async fn failed_saved_value_rollback_is_reported_without_claiming_the_new_shortcut_is_active() {
    let fixture = Fixture::new(Some("old"));
    {
        let mut data = fixture.backend.data.lock().unwrap();
        data.cannot_unregister.insert("old".into());
        data.saves.extend([SaveOutcome::Success, SaveOutcome::Fail]);
    }
    let error = fixture.set(Some("new")).await.unwrap_err();
    assert!(error.contains("unregistration refused"));
    assert!(error.contains("rollback failed"));
    assert!(error.contains("saved shortcut"));
    assert_eq!(fixture.current.lock().unwrap().as_deref(), Some("old"));
    let data = fixture.backend.data.lock().unwrap();
    assert_eq!(data.saved.as_deref(), Some("new"));
    assert_eq!(data.registered, BTreeSet::from(["old".into()]));
}

#[tokio::test]
async fn failed_disable_persistence_keeps_the_old_shortcut_registered() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().saves.push_back(SaveOutcome::Fail);
    assert!(fixture.set(None).await.is_err());
    fixture.assert_state(Some("old"));
    assert!(!fixture.backend.data.lock().unwrap().calls.contains(&"unregister:old".into()));
}

#[tokio::test]
async fn failed_disable_unregistration_restores_the_saved_value() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().cannot_unregister.insert("old".into());
    assert!(fixture.set(None).await.is_err());
    fixture.assert_state(Some("old"));
}

#[tokio::test]
async fn whitespace_disables_the_shortcut_after_persistence_succeeds() {
    let fixture = Fixture::new(Some("old"));
    fixture.set(Some(" \t ")).await.unwrap();
    fixture.assert_state(None);
    assert_eq!(fixture.backend.data.lock().unwrap().calls, ["save:<none>", "unregister:old"]);
}

#[tokio::test]
async fn equivalent_accelerators_do_not_unregister_or_reregister() {
    let fixture = Fixture::new(Some("old"));
    fixture.set(Some("old")).await.unwrap();
    fixture.set(Some("alias-old")).await.unwrap();
    assert_eq!(fixture.current.lock().unwrap().as_deref(), Some("alias-old"));
    let data = fixture.backend.data.lock().unwrap();
    assert_eq!(data.saved.as_deref(), Some("alias-old"));
    assert_eq!(data.registered, BTreeSet::from(["old".into()]));
    assert_eq!(data.calls, ["save:old", "save:alias-old"]);
}

#[tokio::test]
async fn failed_first_enable_leaves_no_shortcut_registered() {
    let fixture = Fixture::new(None);
    fixture.backend.data.lock().unwrap().saves.push_back(SaveOutcome::Fail);
    assert!(fixture.set(Some("new")).await.is_err());
    fixture.assert_state(None);
}

#[tokio::test]
async fn saved_value_read_failure_does_not_touch_registrations() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.data.lock().unwrap().read_fails = true;
    assert!(fixture.set(Some("new")).await.unwrap_err().contains("read failed"));
    fixture.assert_state(Some("old"));
    assert!(fixture.backend.data.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn concurrent_replacements_wait_through_persistence_and_rollback() {
    let fixture = Fixture::new(Some("old"));
    fixture.backend.pause_save.store(true, Ordering::SeqCst);
    fixture.backend.data.lock().unwrap().saves.push_back(SaveOutcome::Fail);
    let first = fixture.set(Some("new"));
    let second = fixture.set(Some("next"));
    fn assert_send<T: Send>(_: &T) {}
    assert_send(&first);
    assert_send(&second);
    tokio::pin!(first, second);

    assert!(futures::poll!(&mut first).is_pending());
    assert!(futures::poll!(&mut second).is_pending());
    assert_eq!(fixture.current.try_lock().unwrap().as_deref(), Some("old"),
        "the synchronous shortcut handler must not be blocked during persistence");
    assert_eq!(fixture.backend.data.lock().unwrap().calls, ["register:new"]);

    fixture.backend.resume.notify_one();
    assert!(first.await.is_err());
    second.await.unwrap();
    fixture.assert_state(Some("next"));
    let calls = &fixture.backend.data.lock().unwrap().calls;
    assert!(calls.iter().position(|c| c == "unregister:new").unwrap()
        < calls.iter().position(|c| c == "register:next").unwrap());
}
