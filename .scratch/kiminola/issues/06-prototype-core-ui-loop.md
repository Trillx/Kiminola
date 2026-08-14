# Prototype the core UI loop

Type: prototype
Status: resolved

## Question

What should Kiminola's core UI loop look and feel like? Build a cheap, rough prototype (stack-agnostic — plain HTML/JS mock is fine) covering:

- Idle/meeting-list screen
- Recording screen: live streaming transcript with "You"/"Others" channel labels, optional notepad
- Post-meeting screen: enhanced notes (baseline summary, merged notepad), raw transcript access

Iterate with the user against the /prototype skill until the loop feels right. The prototype is a throwaway artifact to raise discussion fidelity — link it from this ticket.

## Prototype

File: [prototypes/core-ui-loop/index.html](../prototypes/core-ui-loop/index.html)

Three radically different UI variants of the same core loop, switchable via `?variant=A`, `?variant=B`, or `?variant=C` (or use the floating bottom bar):

- **Variant A — Minimal single-column**: Granola-like. One vertical column, lots of whitespace, simple meeting cards, transcript as plain labeled lines, notepad below transcript during recording.
- **Variant B — Sidebar dashboard**: Persistent left sidebar meeting list, two-panel recording view (live transcript | notepad), post-meeting notes as a grid of cards.
- **Variant C — Conversation bubbles**: Chat-style transcript bubbles, floating note tray, big circular stop FAB, card-based post-meeting summary.

To run: open `prototypes/core-ui-loop/index.html` in a browser. Use the arrows at the bottom (or left/right keyboard keys) to switch variants; click through the screens within each variant.

## Blend exploration

File: [prototypes/core-ui-loop/blend.html](../prototypes/core-ui-loop/blend.html)

A single-screen throwaway prototype that blends **Granola's** calm, stationery-first minimalism (paper-white canvas, warm cream surfaces, single olive accent, fully-pill buttons, generous whitespace) with **Wispr Flow's** instant, fluid voice feel (subtle live waveform, streaming transcript with a partial-line cursor, low-friction chrome).

Design direction:

- Sidebar dashboard shell: left nav (Home, Shared, Chat, Spaces) + main content area with "Coming up" / recent meetings.
- **Recording view is notes-first**: the entire screen is a large sketch notepad; the live transcript is tucked into a subtle bottom-left pill that expands into a small floating square when clicked and collapses back when closed.
- **Enhance is optional**: after stopping, the default view is **My notes**; the user can switch to **Enhance Notes** (which shows a prompt to run AI enhancement) or **Transcript**.
- Post-meeting pill order: **My notes → Enhance Notes → Transcript**.
- Interactive: click any recent meeting or "New meeting" to move through the loop; simulated live transcript streams in during recording.
- Theme toggle lives in the top navigation bar (always visible, even when sidebar is collapsed).
- Includes a **fully collapsible sidebar** controlled by a floating edge button; state persists in localStorage.
- Sidebar Spaces are an expandable tree (e.g., Personal / Work with meetings nested underneath).
- Removed out-of-scope features from the prototype: Invite, Shared with me, Chat.
- Bottom "Ask anything" bar is centered within the main content column, not the full viewport.
- Opening the live transcript pushes the main content slightly right so it never overlaps the notepad; transcript auto-scrolls to new lines and hides its scrollbar until the user scrolls.

Open questions to consider:

- Should the transcript floating square auto-open when speech starts, or stay hidden until clicked?
- Does the amber accent feel on-brand for Kiminola, or should it shift?
- Is the waveform too active for long meetings, or does it read as reassuring feedback?
- Should the "Enhance Notes" prompt disappear after first use, or remain available for re-enhancement?

## Resolution

Resolved on 2026-08-13 after iterating with the user on `prototypes/core-ui-loop/blend.html`.

### Decided UI direction

- **Shell**: Left sidebar + main content area. Sidebar collapses fully via a floating edge button; state persists in localStorage.
- **Navigation**: Home only in sidebar. Spaces is an expandable tree (e.g., Personal / Work) with meetings nested underneath.
- **Removed from prototype**: Invite, Shared with me, Chat — out of scope for MVP.
- **Top bar**: "New meeting" primary action + light/dark mode toggle.
- **Idle / list screen**: "Coming up" header, date card, recent meetings list, bottom "Ask anything" bar.
- **Recording screen**: Notes-first — the full screen is a sketch notepad. A subtle "Live transcript" pill sits bottom-left; clicking it opens a small floating square that pushes the notepad right so the user can keep typing. Transcript auto-scrolls and hides scrollbar until the user scrolls. Stop button says "Stop meeting" (not "Stop & enhance").
- **Post-meeting screen**: Default tab is **My notes**. Pill order: **My notes → Enhance Notes → Transcript**. Enhance Notes starts as a prompt with a button; clicking it simulates AI enhancement. Bottom "Ask anything" bar present.
- **Theming**: Light/dark mode toggle in the top bar; dark mode uses warm amber accent on deep charcoal canvas.

### Prototype assets

- `prototypes/core-ui-loop/index.html` — original three-variant exploration
- `prototypes/core-ui-loop/blend.html` — final blended direction (Granola calm × Wispr Flow fluidity)
