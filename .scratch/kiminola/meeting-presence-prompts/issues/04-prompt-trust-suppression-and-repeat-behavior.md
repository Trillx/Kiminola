# Prompt trust, suppression, and repeat behavior

Type: grilling
Status: resolved
Claimed by: Codex
Blocked by: None

## Question

How should Kiminola prevent Meeting prompts from becoming noisy or surprising? Decide the default notification frequency, snooze and dismiss behavior, per-app allow/ignore controls, focus or presentation-mode suppression, and the wording that makes clear no audio is being recorded until the user chooses to start a Meeting.

## Resolution

Kiminola shows at most one Meeting prompt per detected app session. **Not now** suppresses further prompts until that session ends. The user may also choose an optional **Snooze** action or **Don't ask again for this app** control.

Focus/presentation-mode suppression, accessibility behavior, and final wording remain open for a follow-on ticket.
