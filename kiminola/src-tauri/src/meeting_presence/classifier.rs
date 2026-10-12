//! Pure Meeting presence classifier: turns one local snapshot of processes,
//! visible windows and active Core Audio sessions into meeting candidates.
//!
//! A *detection* (likely) needs active audio in an app family plus either a
//! known meeting app (*targeted*) or a visible window of a generic app family
//! whose input **and** output are both active. A *possible hint* is a known
//! app or browser that is merely running, or a generic audio app that does
//! not (yet) qualify. Results are attributed to a stable app-family root
//! process (the process-loopback capture target) plus the window that best
//! represents the call. Our own processes, malformed process trees, recycled
//! parent PIDs and exited processes never produce candidates, and labels never
//! expose unknown process names.
//!
//! The snapshot also reports per-process creation times (to detect PID reuse),
//! every app family with active audio (episode activity is independent of
//! window visibility), and whether the audio scan was complete enough that a
//! family's absence can be read as confirmed inactivity.

use std::collections::{HashMap, HashSet};

use super::{MeetingPresenceConfidence, MeetingPresenceEvidence};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detection {
    pub(crate) process_id: u32,
    /// A recognized meeting app (rather than a generic audio app).
    pub(crate) targeted: bool,
    pub(crate) window_process_id: Option<u32>,
    pub(crate) app_label: String,
    pub(crate) confidence: MeetingPresenceConfidence,
    pub(crate) evidence: Vec<MeetingPresenceEvidence>,
}

#[derive(Debug)]
pub(crate) struct DetectionSnapshot {
    pub(crate) detections: Vec<Detection>,
    pub(crate) possible_hints: Vec<Detection>,
    /// Every live PID with its creation time when readable.
    pub(crate) live_process_instances: HashMap<u32, Option<u64>>,
    /// App-family roots with any active audio, visible window or not.
    pub(crate) active_audio_process_ids: HashSet<u32>,
    /// False when missing audio/process metadata means absent activity is
    /// unknown rather than confirmed silence.
    pub(crate) audio_scan_complete: bool,
}

impl Default for DetectionSnapshot {
    fn default() -> Self {
        Self {
            detections: Vec::new(),
            possible_hints: Vec::new(),
            live_process_instances: HashMap::new(),
            active_audio_process_ids: HashSet::new(),
            audio_scan_complete: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfo {
    pub(crate) name: String,
    pub(crate) parent_id: u32,
    pub(crate) identity: Option<ProcessIdentity>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessIdentity {
    // Transient ownership evidence only; never emitted, logged, or persisted.
    pub(crate) executable_path: String,
    pub(crate) started_at: u64,
}

/// Running sessions on input/output endpoints, never audio samples. Endpoint
/// direction is evidence, not proof of a physical mic or remote conversation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AudioActivity {
    pub(crate) input: bool,
    pub(crate) output: bool,
}

impl AudioActivity {
    pub(crate) fn merge(&mut self, other: Self) {
        self.input |= other.input;
        self.output |= other.output;
    }
}

pub(crate) fn classify_windows(
    processes: HashMap<u32, ProcessInfo>,
    windows: HashMap<u32, Vec<String>>,
    active_audio: HashMap<u32, AudioActivity>,
    current_pid: u32,
) -> DetectionSnapshot {
    let mut detections = Vec::new();
    let mut possible_hints = Vec::new();
    let mut possible_seen = HashSet::new();
    let mut family_audio = HashMap::<u32, AudioActivity>::new();
    let mut audio_scan_complete = true;

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

    for (pid, activity) in active_audio {
        if !activity.input && !activity.output {
            continue;
        }
        let lineage = process_lineage(pid, &processes);
        let own_audio = lineage
            .iter()
            .any(|id| *id == current_pid || is_our_process(&processes[id].name));
        let detection = resolve_app(pid, &processes, &windows, current_pid);
        if let Some(detection) = detection {
            let attributed_lineage = lineage
                .iter()
                .position(|id| *id == detection.process_id)
                .map(|root_index| &lineage[..=root_index]);
            if !own_audio
                && attributed_lineage
                    .is_none_or(|ids| ids.iter().any(|id| processes[id].identity.is_none()))
            {
                // Only ancestry required to reach the attributed family affects
                // completeness. An unreadable launcher above the root does not.
                audio_scan_complete = false;
            }
            family_audio
                .entry(detection.process_id)
                .or_default()
                .merge(activity);
            // Visibility controls prompt eligibility, not episode activity.
            if detection.targeted || detection.window_process_id.is_some() {
                detections.push(detection);
            }
        } else if !own_audio
            && (lineage.is_empty() || lineage.iter().any(|id| processes[id].identity.is_none()))
        {
            // Missing process metadata may explain failed attribution. Do not
            // interpret the family's absence as confirmed episode inactivity.
            audio_scan_complete = false;
        }
    }
    // A call window on an active helper's branch is more specific than the
    // app's home window. Resolve competing audio sessions deterministically.
    detections.sort_by_key(|d| {
        (
            d.process_id,
            !d.targeted,
            d.window_process_id.is_none(),
            d.window_process_id == Some(d.process_id),
            d.window_process_id.unwrap_or(u32::MAX),
        )
    });
    detections.dedup_by_key(|d| d.process_id);
    detections.retain(|detection| {
        if detection.targeted {
            return true;
        }
        // Unknown apps do not get a visible prompt for music or dictation
        // alone. Input and output may belong to different helpers/devices.
        let activity = family_audio[&detection.process_id];
        let mut hint = detection.clone();
        hint.confidence = MeetingPresenceConfidence::Possible;
        possible_hints.push(hint);
        activity.input && activity.output
    });
    // Hash-map iteration must not randomly change which app gets the prompt.
    detections.sort_by_key(|d| (!d.targeted, d.process_id));
    possible_hints.sort_by_key(|d| d.process_id);
    DetectionSnapshot {
        detections,
        possible_hints,
        live_process_instances: processes
            .iter()
            .map(|(&pid, process)| {
                (
                    pid,
                    process
                        .identity
                        .as_ref()
                        .map(|identity| identity.started_at),
                )
            })
            .collect(),
        active_audio_process_ids: family_audio.keys().copied().collect(),
        audio_scan_complete,
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
        if let (Some(child), Some(parent)) = (
            process.identity.as_ref(),
            processes
                .get(&process.parent_id)
                .and_then(|p| p.identity.as_ref()),
        ) {
            if parent.started_at > child.started_at {
                // The old parent exited and Windows reused its PID.
                next = 0;
                break;
            }
        }
        next = process.parent_id;
    }
    if next != 0 && processes.contains_key(&next) {
        // A cyclic or unreasonably deep snapshot cannot establish ownership.
        return Vec::new();
    }
    lineage
}

fn same_executable(a: &ProcessInfo, b: &ProcessInfo) -> bool {
    a.identity
        .as_ref()
        .zip(b.identity.as_ref())
        .is_some_and(|(a, b)| a.executable_path.eq_ignore_ascii_case(&b.executable_path))
}

fn generic_family_root(pid: u32, processes: &HashMap<u32, ProcessInfo>) -> Option<u32> {
    let lineage = process_lineage(pid, processes);
    let owner_index = lineage
        .iter()
        .position(|id| normalized_process_name(&processes[id].name) != "msedgewebview2")?;
    // A WebView can inherit its directly associated application, not an
    // arbitrary visible shell. Missing identity remains unattributed.
    if lineage[..=owner_index]
        .iter()
        .any(|id| processes[id].identity.is_none())
        && owner_index != 0
    {
        return None;
    }
    let mut root = lineage[owner_index];
    if matches!(
        normalized_process_name(&processes[&root].name).as_str(),
        "explorer"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "conhost"
            | "windowsterminal"
            | "applicationframehost"
            | "svchost"
            | "rundll32"
    ) {
        return None;
    }
    for &parent in &lineage[owner_index + 1..] {
        if !same_executable(&processes[&root], &processes[&parent]) {
            break;
        }
        root = parent;
    }
    Some(root)
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
        .any(|id| *id == current_pid || is_our_process(&processes[id].name))
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
        (root, window_pid)
    } else {
        let root = generic_family_root(pid, processes)?;
        let window_pid = windows
            .keys()
            .filter(|id| has_window(id) && generic_family_root(**id, processes) == Some(root))
            .copied()
            .min();
        (root, window_pid)
    };
    let label_pid = window_pid.unwrap_or(root);
    let titles = windows.get(&label_pid).map(Vec::as_slice).unwrap_or(&[]);
    Some(Detection {
        process_id: root,
        targeted: is_known_meeting_process(&processes[&root].name),
        window_process_id: window_pid,
        app_label: friendly_app_label(&processes[&label_pid].name, titles).into(),
        confidence: MeetingPresenceConfidence::Likely,
        evidence: vec![
            MeetingPresenceEvidence::AppOrVisibleWindow,
            MeetingPresenceEvidence::ActiveCoreAudio,
        ],
    })
}

pub(crate) fn normalized_process_name(process_name: &str) -> String {
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

fn is_our_process(process_name: &str) -> bool {
    let name = normalized_process_name(process_name);
    name.contains("oatwave") || name.contains("kiminola")
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
        "chrome" | "msedge" | "firefox" | "brave"
            if window_titles.iter().any(|title| {
                let title = title.to_ascii_lowercase();
                title.contains("google meet") || title.contains("meet.google.com")
            }) =>
        {
            "Google Meet"
        }
        "chrome" | "msedge" | "firefox" | "brave" => "another app",
        _ => "another app",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        classify_windows, friendly_app_label, generic_family_root, is_known_meeting_process,
        AudioActivity, ProcessIdentity, ProcessInfo,
    };
    use crate::meeting_presence::probe::{detect_audio_fixture, detect_fixture};
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
    fn unknown_apps_require_input_and_output_from_the_same_family() {
        let processes = &[(10, 0, "call-app.exe"), (20, 0, "player.exe")];
        let windows = &[(10, "Call"), (20, "Player")];
        for audio in [
            vec![(10, false, true)],
            vec![(10, true, false)],
            vec![(10, true, false), (20, false, true)],
        ] {
            let snapshot = detect_audio_fixture(processes, windows, &audio);
            assert!(
                snapshot.detections.is_empty(),
                "one-way/unrelated audio must stay quiet"
            );
        }
        let snapshot = detect_audio_fixture(processes, windows, &[(10, true, true)]);
        assert_eq!(snapshot.detections.len(), 1);
        assert_eq!(snapshot.detections[0].app_label, "another app");
        let known = detect_audio_fixture(&[(30, 0, "ms-teams.exe")], &[], &[(30, false, true)]);
        assert_eq!(
            known.detections.len(),
            1,
            "recognized apps retain targeted detection"
        );
    }

    #[test]
    fn generic_helpers_combine_directions_only_under_their_visible_owner() {
        let processes = &[
            (1, 0, "explorer.exe"),
            (10, 1, "call-app.exe"),
            (11, 10, "call-app.exe"),
            (12, 10, "msedgewebview2.exe"),
            (20, 1, "call-app.exe"),
            (21, 20, "call-app.exe"),
        ];
        let windows = &[(1, "Desktop"), (10, "Call"), (20, "Other instance")];
        let together =
            detect_audio_fixture(processes, windows, &[(11, true, false), (12, false, true)]);
        assert_eq!(together.detections.len(), 1);
        assert_eq!(together.detections[0].process_id, 10);
        assert_eq!(together.detections[0].window_process_id, Some(10));
        let separate =
            detect_audio_fixture(processes, windows, &[(11, true, false), (21, false, true)]);
        assert!(separate.detections.is_empty());
        let shell = detect_audio_fixture(
            &[(1, 0, "explorer.exe"), (12, 1, "msedgewebview2.exe")],
            &[(1, "Desktop")],
            &[(12, true, true)],
        );
        assert!(
            shell.detections.is_empty(),
            "the shell is never a generic call owner"
        );
    }

    #[test]
    fn unreadable_launcher_does_not_make_a_resolved_app_scan_incomplete() {
        let processes = HashMap::from([
            (
                1,
                ProcessInfo {
                    name: "explorer.exe".into(),
                    parent_id: 0,
                    identity: None,
                },
            ),
            (
                10,
                ProcessInfo {
                    name: "call-app.exe".into(),
                    parent_id: 1,
                    identity: Some(ProcessIdentity {
                        executable_path: "C:/Fixture/call-app.exe".into(),
                        started_at: 1,
                    }),
                },
            ),
        ]);
        let snapshot = classify_windows(
            processes,
            HashMap::from([(10, vec!["Call".into()])]),
            HashMap::from([(
                10,
                AudioActivity {
                    input: true,
                    output: true,
                },
            )]),
            999,
        );

        assert_eq!(snapshot.detections.len(), 1);
        assert!(
            snapshot.audio_scan_complete,
            "identity outside the attributed app family must not stall every onset counter"
        );
    }

    #[test]
    fn generic_ownership_requires_identity_and_rejects_recycled_parents() {
        let make_process = |parent_id, path: &str, started_at| ProcessInfo {
            name: "call-app.exe".into(),
            parent_id,
            identity: Some(ProcessIdentity {
                executable_path: path.into(),
                started_at,
            }),
        };
        let mut processes = HashMap::from([
            (10, make_process(0, "C:/One/call-app.exe", 1)),
            (11, make_process(10, "C:/Two/call-app.exe", 2)),
        ]);
        assert_eq!(
            generic_family_root(11, &processes),
            Some(11),
            "matching basenames are not enough to combine audio"
        );
        processes
            .get_mut(&11)
            .unwrap()
            .identity
            .as_mut()
            .unwrap()
            .executable_path = "c:/one/CALL-APP.EXE".into();
        assert_eq!(generic_family_root(11, &processes), Some(10));
        processes
            .get_mut(&10)
            .unwrap()
            .identity
            .as_mut()
            .unwrap()
            .started_at = 3;
        assert_eq!(
            generic_family_root(11, &processes),
            Some(11),
            "a later process cannot own an earlier child"
        );
        processes.get_mut(&10).unwrap().identity = None;
        assert_eq!(generic_family_root(11, &processes), Some(11));
        processes.get_mut(&11).unwrap().name = "msedgewebview2.exe".into();
        assert_eq!(
            generic_family_root(11, &processes),
            None,
            "a WebView with an unverified owner stays unattributed"
        );
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
        assert!(result
            .detections
            .iter()
            .all(|d| d.app_label != "Microsoft Teams"));
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
            &[(999, 0, "oatwave.exe"), (11, 999, "msedgewebview2.exe")],
            &[(999, "OatWave"), (11, "Prompt")],
            &[999, 11, 404],
        );
        assert!(result.detections.is_empty());
        assert!(result.possible_hints.is_empty());
    }

    #[test]
    fn renamed_and_previous_app_processes_never_create_meeting_hints() {
        for executable in ["oatwave.exe", "OatWave.EXE", "kiminola.exe"] {
            let result = detect_fixture(
                &[(888, 0, executable), (11, 888, "msedgewebview2.exe")],
                &[(888, "OatWave"), (11, "Meeting prompt")],
                &[888, 11],
            );
            assert!(result.detections.is_empty(), "{executable}");
            assert!(result.possible_hints.is_empty(), "{executable}");
        }
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
