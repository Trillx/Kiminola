//! Process-wide ownership for Meeting capture, dictation and installation.
//! A lease covers startup through finalization, not just microphone activity.

use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOwner {
    Meeting,
    Dictation,
    Update,
    Quit,
}

#[derive(Default)]
struct GateState {
    owner: Option<CaptureOwner>,
    dictation_review: bool,
    generation: u64,
}

#[derive(Default, Clone)]
pub struct CaptureGate {
    state: Arc<Mutex<GateState>>,
}

pub struct CaptureLease {
    state: Arc<Mutex<GateState>>,
    generation: u64,
}

impl CaptureGate {
    pub fn claim(&self, owner: CaptureOwner) -> Result<CaptureLease, String> {
        self.claim_inner(owner, false)
    }

    /// Only an explicit Copy/Discard/Confirm exit decision may consume review.
    /// Failure leaves review untouched; success never exposes an idle gap to Update.
    pub fn claim_quit_after_review_resolved(&self) -> Result<CaptureLease, String> {
        self.claim_inner(CaptureOwner::Quit, true)
    }

    fn claim_inner(
        &self,
        owner: CaptureOwner,
        review_resolved: bool,
    ) -> Result<CaptureLease, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Capture ownership is unavailable.")?;
        if let Some(active) = state.owner {
            return Err(match active {
                CaptureOwner::Meeting => {
                    "Finish the current Meeting before starting dictation or an update."
                }
                CaptureOwner::Dictation => {
                    "Finish or cancel dictation before starting another capture or an update."
                }
                CaptureOwner::Update => {
                    "The app is preparing to update. Try again after it restarts."
                }
                CaptureOwner::Quit => "The app is quitting. New capture cannot start.",
            }
            .into());
        }
        if state.dictation_review && owner != CaptureOwner::Meeting && !review_resolved {
            return Err("Copy or dismiss the pending dictation text before starting dictation or an update.".into());
        }
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or("Capture session limit reached. Restart the app.")?;
        state.owner = Some(owner);
        if review_resolved {
            state.dictation_review = false;
        }
        Ok(CaptureLease {
            state: Arc::clone(&self.state),
            generation: state.generation,
        })
    }

    pub fn set_dictation_review(&self, pending: bool) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .dictation_review = pending;
    }

    #[cfg(test)]
    pub fn has_dictation_review(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.dictation_review)
            .unwrap_or(true)
    }

    pub fn is_busy(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.owner.is_some())
            .unwrap_or(true)
    }
}

impl Drop for CaptureLease {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.generation == self.generation {
            state.owner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_startup_and_pause_keep_dictation_and_update_excluded() {
        let gate = CaptureGate::default();
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        assert!(gate.claim(CaptureOwner::Dictation).is_err());
        assert!(gate.claim(CaptureOwner::Update).is_err());
        drop(meeting);
        assert!(gate.claim(CaptureOwner::Dictation).is_ok());
    }

    #[test]
    fn recovery_blocks_dictation_and_update_but_allows_meetings() {
        let gate = CaptureGate::default();
        let dictation = gate.claim(CaptureOwner::Dictation).unwrap();
        gate.set_dictation_review(true);
        drop(dictation);
        assert!(gate.claim(CaptureOwner::Dictation).is_err());
        assert!(gate.claim(CaptureOwner::Update).is_err());
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        drop(meeting);
        gate.set_dictation_review(false);
        assert!(gate.claim(CaptureOwner::Update).is_ok());
    }

    #[test]
    fn simultaneous_meeting_dictation_and_update_have_one_owner() {
        let gate = CaptureGate::default();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = [
                CaptureOwner::Meeting,
                CaptureOwner::Dictation,
                CaptureOwner::Update,
            ]
            .into_iter()
            .map(|owner| {
                let gate = gate.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    let lease = gate.claim(owner);
                    barrier.wait();
                    lease
                })
            })
            .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|lease| lease.is_ok()).count(), 1);
        assert!(gate.is_busy());
        drop(results);
        assert!(!gate.is_busy());
    }

    #[test]
    fn failed_resolved_review_claim_preserves_review_even_at_generation_limit() {
        let gate = CaptureGate::default();
        gate.set_dictation_review(true);
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        assert!(gate.claim_quit_after_review_resolved().is_err());
        assert!(gate.has_dictation_review());
        drop(meeting);
        gate.state.lock().unwrap().generation = u64::MAX;
        assert!(gate.claim_quit_after_review_resolved().is_err());
        assert!(gate.has_dictation_review());
        assert!(!gate.is_busy());
    }

    #[test]
    fn delayed_quit_cannot_exit_a_new_meeting_or_leave_recovery_unresolved() {
        let gate = CaptureGate::default();
        gate.set_dictation_review(true);
        assert!(gate.claim(CaptureOwner::Quit).is_err());
        let meeting = gate.claim(CaptureOwner::Meeting).unwrap();
        gate.set_dictation_review(false);
        assert!(gate.claim(CaptureOwner::Quit).is_err());
        drop(meeting);
        let quit = gate.claim(CaptureOwner::Quit).unwrap();
        assert!(gate.claim(CaptureOwner::Meeting).is_err());
        assert!(gate.claim(CaptureOwner::Dictation).is_err());
        assert!(gate.claim(CaptureOwner::Update).is_err());
        drop(quit);
    }
}
