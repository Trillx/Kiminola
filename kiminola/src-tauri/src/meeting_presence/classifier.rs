//! Pure Meeting presence classifier: turns one local snapshot of processes,
//! visible windows and active Core Audio sessions into meeting candidates.
//!
//! A *detection* (likely) needs a known meeting app (or a visible window of
//! the audio process itself) **and** active audio in that app's process
//! family. A *possible hint* is a known app or browser that is merely running.
//! Results are attributed to a stable app-family root process (the
//! process-loopback capture target) plus the window that best represents the
//! call. Our own processes, malformed process trees and exited processes never
//! produce candidates, and labels never expose unknown process names.

use std::collections::{HashMap, HashSet};

use super::{MeetingPresenceConfidence, MeetingPresenceEvidence};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detection {
    pub(crate) process_id: u32,
    pub(crate) window_process_id: Option<u32>,
    pub(crate) app_label: String,
    pub(crate) confidence: MeetingPresenceConfidence,
    pub(crate) evidence: Vec<MeetingPresenceEvidence>,
}

#[derive(Debug, Default)]
pub(crate) struct DetectionSnapshot {
    pub(crate) detections: Vec<Detection>,
    pub(crate) possible_hints: Vec<Detection>,
    pub(crate) live_process_ids: HashSet<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfo {
    pub(crate) name: String,
    pub(crate) parent_id: u32,
}

pub(crate) fn classify_windows(
    processes: HashMap<u32, ProcessInfo>,
    windows: HashMap<u32, Vec<String>>,
    active_audio: HashSet<u32>,
    current_pid: u32,
) -> DetectionSnapshot {
    let mut detections = Vec::new();
    let mut possible_hints = Vec::new();
    let mut possible_seen = HashSet::new();

    for (&pid, process) in &processes {
        // Only named apps contribute quiet hints. Audio candidates additionally
        // allow the spec's visible-window "another app" fallback.
        if !is_known_meeting_process(&process.name) && !is_browser_process(&process.name) {
            continue;
        }
        if let Some(mut detection) = resolve_app(pid, &processes, &windows, current_pid) {
            if detection.app_label != "another app" && possible_seen.insert(detection.process_id) {
                detection.confidence = MeetingPresenceConfidence::Possible;
                detection.evidence = vec![MeetingPresenceEvidence::AppOrVisibleWindow];
                possible_hints.push(detection);
            }
        }
    }

    for pid in active_audio {
        if let Some(detection) = resolve_app(pid, &processes, &windows, current_pid) {
            detections.push(detection);
        }
    }
    // A call window on an active helper's branch is more specific than the
    // app's home window. Resolve competing audio sessions deterministically.
    detections.sort_by_key(|d| {
        (
            d.process_id,
            d.app_label == "another app",
            d.window_process_id.is_none(),
            d.window_process_id == Some(d.process_id),
            d.window_process_id.unwrap_or(u32::MAX),
        )
    });
    detections.dedup_by_key(|d| d.process_id);
    // Hash-map iteration must not randomly change which app gets the prompt.
    detections.sort_by_key(|d| (d.app_label == "another app", d.process_id));
    possible_hints.sort_by_key(|d| d.process_id);
    DetectionSnapshot {
        detections,
        possible_hints,
        live_process_ids: processes.keys().copied().collect(),
    }
}

fn process_lineage(pid: u32, processes: &HashMap<u32, ProcessInfo>) -> Vec<u32> {
    let mut lineage = Vec::new();
    let mut next = pid;
    while next != 0 && lineage.len() < 64 && !lineage.contains(&next) {
        let Some(process) = processes.get(&next) else {
            break;
        };
        lineage.push(next);
        next = process.parent_id;
    }
    if next != 0 && processes.contains_key(&next) {
        // A cyclic or unreasonably deep snapshot cannot establish ownership.
        return Vec::new();
    }
    lineage
}

fn resolve_app(
    pid: u32,
    processes: &HashMap<u32, ProcessInfo>,
    windows: &HashMap<u32, Vec<String>>,
    current_pid: u32,
) -> Option<Detection> {
    let lineage = process_lineage(pid, processes);
    if lineage.is_empty() {
        return None;
    }
    if lineage
        .iter()
        .any(|id| *id == current_pid || is_kiminola_process(&processes[id].name))
    {
        return None;
    }
    let has_window = |id: &u32| windows.get(id).is_some_and(|titles| !titles.is_empty());
    // Embedded WebView helpers inherit their meeting app. A standalone browser
    // is its own boundary, even if a meeting app launched it to open a link.
    let owner = lineage.iter().find(|id| {
        is_known_meeting_process(&processes[id].name) || is_browser_process(&processes[id].name)
    });
    let (root, window_pid) = if let Some(&owner) = owner {
        let name = &processes[&owner].name;
        let same_family = |id: &u32| {
            let candidate = &processes[id].name;
            if is_known_meeting_process(name) {
                is_known_meeting_process(candidate)
                    && friendly_app_label(candidate, &[]) == friendly_app_label(name, &[])
            } else {
                normalized_process_name(candidate) == normalized_process_name(name)
            }
        };
        let root = lineage.iter().rev().find(|id| same_family(id)).copied()?;
        let window_pid =
            if let Some(window_pid) = lineage.iter().find(|id| has_window(id) && same_family(id)) {
                Some(*window_pid)
            } else {
                windows
                    .keys()
                    .filter(|id| {
                        processes.contains_key(id)
                            && has_window(id)
                            && same_family(id)
                            && process_lineage(**id, processes).contains(&root)
                    })
                    .copied()
                    .min()
            };
        if window_pid.is_none() && !is_known_meeting_process(name) {
            return None;
        }
        (root, window_pid)
    } else {
        // Preserve the generic fallback only for the actual audio process's
        // window, never an arbitrary parent such as Explorer.
        if !has_window(&pid) {
            return None;
        }
        (pid, Some(pid))
    };
    let label_pid = window_pid.unwrap_or(root);
    let titles = windows.get(&label_pid).map(Vec::as_slice).unwrap_or(&[]);
    Some(Detection {
        process_id: root,
        window_process_id: window_pid,
        app_label: friendly_app_label(&processes[&label_pid].name, titles).into(),
        confidence: MeetingPresenceConfidence::Likely,
        evidence: vec![
            MeetingPresenceEvidence::AppOrVisibleWindow,
            MeetingPresenceEvidence::ActiveCoreAudio,
        ],
    })
}

fn normalized_process_name(process_name: &str) -> String {
    process_name
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(process_name)
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_string()
}

fn is_browser_process(process_name: &str) -> bool {
    matches!(
        normalized_process_name(process_name).as_str(),
        "chrome" | "msedge" | "firefox" | "brave"
    )
}

fn is_kiminola_process(process_name: &str) -> bool {
    normalized_process_name(process_name).contains("kiminola")
}

pub(crate) fn is_known_meeting_process(process_name: &str) -> bool {
    let name = normalized_process_name(process_name);
    matches!(
        name.as_str(),
        "granola"
            | "zoom"
            | "zoomphone"
            | "teams"
            | "ms-teams"
            | "msteams"
            | "webex"
            | "webexmta"
            | "webexhost"
    )
}

pub(crate) fn friendly_app_label(process_name: &str, window_titles: &[String]) -> &'static str {
    let name = normalized_process_name(process_name);
    match name.as_str() {
        "granola" => "Granola",
        "zoom" | "zoomphone" => "Zoom",
        "teams" | "ms-teams" | "msteams" => "Microsoft Teams",
        "webex" | "webexmta" | "webexhost" => "Webex",
        "chrome" | "msedge" | "firefox" | "brave" => {
            if window_titles.iter().any(|title| {
                let title = title.to_ascii_lowercase();
                title.contains("google meet") || title.contains("meet.google.com")
            }) {
                "Google Meet"
            } else {
                "another app"
            }
        }
        _ => "another app",
    }
}

#[cfg(test)]
mod tests {
    use super::{friendly_app_label, is_known_meeting_process};
    use crate::meeting_presence::probe::detect_fixture;
    use crate::meeting_presence::MeetingPresenceConfidence;

    #[test]
    fn teams_helper_audio_targets_the_visible_meeting_app() {
        let result = detect_fixture(
            &[(10, 0, "ms-teams.exe"), (11, 10, "msedgewebview2.exe")],
            &[(10, "Microsoft Teams")],
            &[11],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].process_id, 10);
        assert_eq!(result.detections[0].window_process_id, Some(10));
        assert_eq!(result.detections[0].app_label, "Microsoft Teams");
    }

    #[test]
    fn zoom_sibling_window_and_audio_share_one_capture_root() {
        let result = detect_fixture(
            &[
                (10, 0, "Zoom.exe"),
                (11, 10, "Zoom.exe"),
                (12, 10, "Zoom.exe"),
            ],
            &[(11, "Zoom Meeting")],
            &[10, 12],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].process_id, 10);
        assert_eq!(result.detections[0].window_process_id, Some(11));
        assert_eq!(result.detections[0].app_label, "Zoom");
    }

    #[test]
    fn call_window_on_the_audio_branch_wins_over_the_app_home_window() {
        for _ in 0..16 {
            let result = detect_fixture(
                &[
                    (10, 0, "ms-teams.exe"),
                    (11, 10, "ms-teams.exe"),
                    (12, 11, "msedgewebview2.exe"),
                ],
                &[(10, "Teams Home"), (11, "Meeting")],
                &[10, 12],
            );
            assert_eq!(result.detections.len(), 1);
            assert_eq!(result.detections[0].process_id, 10);
            assert_eq!(result.detections[0].window_process_id, Some(11));
        }
    }

    #[test]
    fn opening_teams_without_audio_only_produces_a_quiet_hint() {
        let result = detect_fixture(&[(10, 0, "ms-teams.exe")], &[(10, "Teams")], &[]);
        assert!(result.detections.is_empty());
        assert_eq!(result.possible_hints.len(), 1);
        assert_eq!(
            result.possible_hints[0].confidence,
            MeetingPresenceConfidence::Possible
        );
    }

    #[test]
    fn uppercase_executable_paths_are_recognized() {
        let result = detect_fixture(&[(10, 0, "C:\\Apps\\MS-TEAMS.EXE")], &[], &[10]);
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].app_label, "Microsoft Teams");
    }

    #[test]
    fn browser_audio_helper_resolves_to_google_meet_window() {
        let result = detect_fixture(
            &[(10, 0, "chrome.exe"), (11, 10, "chrome.exe")],
            &[(10, "Google Meet - Planning")],
            &[11],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].app_label, "Google Meet");
        assert_eq!(result.detections[0].window_process_id, Some(10));
    }

    #[test]
    fn unrelated_webview_audio_cannot_activate_idle_teams() {
        let result = detect_fixture(
            &[
                (1, 0, "explorer.exe"),
                (10, 1, "ms-teams.exe"),
                (20, 1, "other.exe"),
                (21, 20, "msedgewebview2.exe"),
            ],
            &[(1, "Desktop"), (10, "Teams"), (20, "Other app")],
            &[21],
        );
        assert!(result.detections.is_empty());
    }

    #[test]
    fn browser_launched_from_teams_does_not_turn_browser_audio_into_a_teams_call() {
        let result = detect_fixture(
            &[
                (10, 0, "ms-teams.exe"),
                (20, 10, "chrome.exe"),
                (21, 20, "chrome.exe"),
            ],
            &[(10, "Teams"), (20, "Video")],
            &[21],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].process_id, 20);
        assert_eq!(result.detections[0].app_label, "another app");
    }

    #[test]
    fn separate_zoom_instances_do_not_share_audio_or_windows() {
        let result = detect_fixture(
            &[
                (10, 0, "Zoom.exe"),
                (20, 0, "Zoom.exe"),
                (21, 20, "Zoom.exe"),
            ],
            &[(10, "Idle Zoom"), (20, "Zoom Meeting")],
            &[21],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].process_id, 20);
        assert_eq!(result.detections[0].window_process_id, Some(20));
    }

    #[test]
    fn our_webview_audio_and_stale_audio_pids_are_ignored() {
        let result = detect_fixture(
            &[(999, 0, "kiminola.exe"), (11, 999, "msedgewebview2.exe")],
            &[(999, "Kimi Nola"), (11, "Prompt")],
            &[999, 11, 404],
        );
        assert!(result.detections.is_empty());
        assert!(result.possible_hints.is_empty());
    }

    #[test]
    fn known_meeting_apps_take_priority_over_generic_audio_windows() {
        let result = detect_fixture(
            &[(1, 0, "player.exe"), (10, 0, "ms-teams.exe")],
            &[(1, "Player"), (10, "Teams")],
            &[1, 10],
        );
        assert_eq!(result.detections.len(), 2);
        assert_eq!(result.detections[0].app_label, "Microsoft Teams");
        assert_eq!(result.detections[1].app_label, "another app");
    }

    #[test]
    fn malformed_process_trees_and_exited_processes_do_not_prompt() {
        let result = detect_fixture(
            &[(10, 11, "Zoom.exe"), (11, 10, "Zoom.exe")],
            &[(10, "Zoom"), (404, "Exited app")],
            &[11, 404],
        );
        assert!(result.detections.is_empty());
        assert!(result.possible_hints.is_empty());
    }

    #[test]
    fn windows_idle_process_is_a_parent_boundary() {
        let result = detect_fixture(
            &[(0, 0, "[System Process]"), (10, 0, "Zoom.exe")],
            &[(10, "Zoom Meeting")],
            &[10],
        );
        assert_eq!(result.detections.len(), 1);
        assert_eq!(result.detections[0].process_id, 10);
    }

    #[test]
    fn known_apps_use_friendly_labels() {
        assert_eq!(friendly_app_label("Granola.exe", &[]), "Granola");
        assert_eq!(friendly_app_label("teams.exe", &[]), "Microsoft Teams");
        assert_eq!(
            friendly_app_label("chrome.exe", &["Google Meet · Project sync".into()]),
            "Google Meet"
        );
    }

    #[test]
    fn unknown_apps_never_expose_process_names() {
        assert_eq!(
            friendly_app_label("some-private-app.exe", &["Room".into()]),
            "another app"
        );
        assert!(!is_known_meeting_process("some-private-app.exe"));
    }
}
