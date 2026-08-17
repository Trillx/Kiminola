# First-run wizard UI prototype

Type: prototype
Status: open
Blocked by: 16, 17, 18

## Question

Prototype the first-run wizard UI so we can validate the look, step flow, and copy before implementation.

The prototype should show:
- Full-screen container that replaces the library until onboarding completes.
- Step indicator: "Step X of 3" with a segmented progress bar.
- Step 1 — Microphone: permission explanation, permission-request button, optional 3-second mic check with a level meter, and a denied-permission error state.
- Step 2 — Model pack: privacy explanation, download progress bar (percentage, MB count, ETA), error/recovery card, and "Open model folder" link for manual install.
- Step 3 — AI Provider: optional BYOK config form (Provider kind, base URL, model, API key) with skip action.
- Completion: transition to library with a brief "Model ready" confirmation.

Use the existing prototype in `.scratch/kiminola/prototypes/core-ui-loop/blend.html` as the visual reference (pill buttons, warm amber accent on charcoal in dark mode, light stationery feel in light mode). Keep it rough and cheap; the goal is to confirm the UX, not ship CSS.

## Resolution

- Built a throwaway interactive prototype at `src/routes/(onboarding)/onboarding/+page.svelte` with an isolated `+layout@.svelte` (no sidebar/topbar).
- Validated step flow: mic permission + optional check, Model pack download with progress/error simulation, optional BYOK Provider form, completion → library.
- Visual approach: full-screen centered card, "Step X of 3" segmented progress bar, one gold element per step (mic meter / progress bar / focus states), Oatwave tokens.
- Decisions confirmed by this prototype:
  - Wizard is its own route with isolated layout.
  - Step indicator is a thin progress track, not numbered dots.
  - Provider step shows a compact form with a prominent Skip action.
  - Error recovery uses stacked buttons: Retry (primary), Open model folder (ghost).
