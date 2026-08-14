# Telemetry and crash-reporting stance

Type: grilling
Status: resolved

## Question

What, if anything, does Kiminola phone home? The product's pitch is "audio never leaves the machine," so the default posture matters:

- Usage analytics: none, or opt-in only (and then what backend?)
- Crash reporting: none, or opt-in (Sentry? simple log-and-report flow?)
- Update checks: the auto-updater inherently contacts a server (details in ticket 10) — accepted as the one exception?

Expected to be a quick confirmation round with the user.

## Resolution

Resolved on 2026-08-13 after a quick confirmation round. The user deferred to the recommendation.

### Decisions

- **Usage analytics — none, ever.** No telemetry, no event tracking, no usage statistics collection in the MVP. This is a privacy-first OSS product; the stance is easy to defend and market.
- **Crash reporting — opt-in only, via Sentry** (or equivalent privacy-respecting backend). Off by default. If the user opts in, the app sends a minimal crash report (stack trace, app version, OS/architecture, no transcript or note content). Sentry's open-source plan is the pragmatic MVP backend; self-hosted or a simpler backend can replace it later without user-visible changes.
- **Update checks — accepted as the one necessary server ping.** The Tauri auto-updater contacts the update endpoint to ask "is there a new version?" This carries no user data beyond the current version and architecture. This is the only automatic server contact when crash reporting is disabled.

### Consequences

- First-run onboarding should surface the crash-reporting opt-in (or keep it in Settings) — no silent enrollment.
- The spec's privacy section can state clearly: "Kiminola does not collect usage analytics. Transcript audio never leaves the machine. Transcript text leaves only when you explicitly choose a cloud LLM for enhancement. Optional crash reporting is the only other outbound connection."
- Crash-reporting backend selection is an implementation detail; the policy (opt-in, minimal, no content) is fixed.
