# Onboarding state and route gating

Type: grilling
Status: open
Blocked by:

## Question

Decide how Kiminola remembers that onboarding is complete and how it enforces the first-run gate.

Specifically:
- Where is **Onboarding state** persisted? (SQLite `settings` table, a separate flag, or a file.)
- What granularity do we store? (A single `onboarding_complete` boolean, or per-step flags.)
- How does the frontend enforce the gate? (SvelteKit route guard, layout redirect, or a Tauri-managed splash/check.)
- What happens if the user deletes the Model pack after onboarding? (Re-run wizard, show a banner, or treat as a different "missing model" flow.)
- Should the onboarding wizard live at its own route (e.g., `/onboarding`) or as a conditional full-screen component inside `+layout.svelte`?

Resolve with the user in a short `/grilling` session. The answer should be simple enough to implement in one agent session.

## Resolution

- **Onboarding state storage**: SQLite `settings` table, a single boolean `onboarding_complete`.
- **Granularity**: one flag. We do not track per-step completion persistently; the wizard is forward-only and short enough to finish in one session.
- **Frontend gate**: SvelteKit `+layout.svelte` checks the flag on mount and redirects any route to `/onboarding` while `onboarding_complete` is false.
- **Wizard location**: own route `/onboarding`, rendered full-screen without the sidebar/topbar shell.
- **Missing model after onboarding**: library shows a dismissible "Model pack missing" banner with a re-download action; the wizard is not auto-restarted.
