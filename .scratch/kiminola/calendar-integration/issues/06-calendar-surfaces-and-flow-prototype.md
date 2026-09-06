# Calendar surfaces and flow prototype

Type: prototype
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

What should the end-to-end calendar interaction feel like in Kimi Nola?

Build a throwaway UI prototype, matching the Oatwave direction, that exercises: Settings account connection and per-calendar selection; sync enabled/disabled and stale/reconnect states; Home upcoming events; starting a Meeting from an event; the event-aware Meeting prompt and explicit confirmation; recording handoff; and the linked event summary on Meeting detail. Include empty, loading, error, all-day, recurring, and competing-event states. The prototype should expose the state after each action and remain clearly non-production until the UX decision is captured.

## Prototype asset

- [Calendar surfaces prototype](../prototypes/calendar-surfaces/index.html) — standalone throwaway HTML with three radically different variants, selected with `?variant=A`, `?variant=B`, or `?variant=C`.
- **A · Home rail** — Kimi Nola's existing Home shell leads, with upcoming events and a compact connection panel.
- **B · Schedule cockpit** — a denser calendar timeline with a selected-event inspector and prompt chooser.
- **C · Prompt first** — a quiet, privacy-forward flow where calendar context appears only at the moment of the Meeting prompt.
- Every variant includes Settings, prompt, detail, stale/reconnect, competing-event, and state-inspector interactions. The prototype is intentionally uncommitted and does not change production routes.

## Resolution

The owner chose **A · Home rail**. The calendar experience should extend the existing Kimi Nola Home surface rather than introduce a full calendar replacement:

- Home remains the primary entry point, with a compact connection/status panel and an upcoming-event list beside the existing recent-Meeting library.
- Settings owns provider connection, account/calendar selection, per-calendar freshness/error state, manual refresh, and the explicit keep-data/delete-data paths for disconnect and deselection.
- A fresh event row can open the Event-aware Meeting prompt; one candidate is shown directly and competing candidates use a chooser. The prompt and recording handoff follow the resolved event-aware contract.
- Meeting detail shows the durable Linked event snapshot as secondary metadata with explicit unlink/relink controls. Standalone Note drafts remain calendar-free.
- The full three-variant prototype remains the primary design asset at `prototypes/calendar-surfaces/index.html`; variants B and C are not carried into the production direction.
