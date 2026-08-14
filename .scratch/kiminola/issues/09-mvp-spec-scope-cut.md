# MVP spec scope cut

Type: grilling
Status: resolved

## Question

Draw the exact MVP boundary for the build-ready spec, with the user (one question at a time). Candidates to rule in or out:

- Meeting history / library, search across transcripts
- Export formats (Markdown, plain text, clipboard, DOCX?)
- Summary templates (fixed set vs customizable prompts)
- Audio recording retention (keep audio? delete after transcription?)
- Transcript editing/correction
- Global hotkey for start/stop
- Onboarding flow

Output: the in/out list that the spec's scope section will state verbatim.

## Resolution

Resolved on 2026-08-13 after a one-question-at-a-time grilling session covering all seven candidates.

### MVP scope — IN

- **Meeting library**: past meetings persist (notes + transcript), browsed via the sidebar list and Spaces tree.
- **Simple search**: one search box, full-text over meeting titles, notes, and transcripts (SQLite FTS5). Semantic/AI search out — it needs an on-device embedding subsystem; upgrade path preserved since transcripts stay on disk.
- **Export**: copy to clipboard + save notes as `.md` / transcript as `.txt`. DOCX/PDF out.
- **Summary templates**: general default + built-in library (1:1, hiring, weekly team, customer discovery, VC pitch, etc.) + user-created custom templates. A template = name + prompt text, managed in settings, picked on the Enhance view. Out: template sharing/marketplace, variables/macros, per-Space defaults.
- **Transcript editing**: inline text editing of transcript segments; enhancement and the search index use the corrected text. Speaker relabeling (You/Others reassignment) out.
- **Global hotkey**: configurable start/stop hotkey with a sensible default (Tauri global-shortcut plugin).
- **Onboarding**: minimal first-run wizard — mic permission → model download with progress → optional BYOK key (skippable). Feature tour / sample content out.

### MVP scope — OUT (post-MVP candidates)

- Semantic/AI search
- Audio retention (MVP never keeps audio — "nothing to leak" privacy story; opt-in retention is a clean post-MVP add since the ring buffer can be dumped to disk later)
- DOCX/PDF export
- Speaker relabeling
- Template sharing/marketplace, template variables/macros, per-Space default templates
- Onboarding feature tour

### Consequences

- Audio is never persisted → transcript editing is the error-correction mechanism (no playback to verify against).
- Library + FTS5 search → transcripts/notes need a real local store; graduated to [ticket 12](12-local-storage-layout.md).
- Enhancement output structure and notepad-merge behavior still open → graduated to [ticket 13](13-enhancement-output-shape.md).
