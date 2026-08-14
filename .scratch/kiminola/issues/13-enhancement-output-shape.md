# Enhancement output shape and notepad merge

Type: grilling
Status: resolved

## Question

What does "Enhance Notes" actually produce? Templates are decided (ticket 09: name + prompt, built-in library + custom). Still open:

- Structure of the enhanced note (what sections, how the template prompt shapes them)
- How the user's notepad merges with the AI output: appended? interleaved? side-by-side? Are "My notes" always preserved verbatim?
- Re-enhancement: does the Enhance prompt remain available after first use (ticket 06 open question)? Overwrite or keep versions?
- Where enhanced output lives relative to the My notes / Enhance Notes / Transcript pills (ticket 06)

Decide with the user, one question at a time; consider a quick /prototype mock for the merge behavior.

## Resolution

Resolved on 2026-08-13 after a one-question-at-a-time grilling session.

### Decisions

- **Merge behavior — side-by-side with raw preserved.** The user's raw notepad stays verbatim in the **My notes** tab. **Enhance Notes** produces a separate polished AI document that uses the raw notes + transcript as context to rewrite, expand, and fill gaps. The original notes are source material, never overwritten.
- **Re-enhancement — overwrite.** Raw notes and transcript are the durable source of truth; the enhanced note is a derived view. Hitting Enhance Notes again (after transcript edits, template switches, or raw-note additions) regenerates the enhanced view, overwriting the previous one.
- **Structure — prompt-defined.** The default template prompts the LLM for a sensible Markdown structure (e.g., Summary → Key points → Action items), and custom templates can define their own headings. The app renders the resulting Markdown faithfully; it does not parse action items into special UI at MVP.
- **Placement & editability — read-only generated view.** My notes = editable raw notepad. Enhance Notes = read-only AI artifact. Transcript = read-only raw transcript. If the user wants changes, they edit the raw notes or transcript and re-enhance.

### Consequences

- The three post-meeting pills from ticket 06 now have unambiguous content: **My notes** (raw), **Enhance Notes** (AI artifact), **Transcript** (source).
- No version history for enhanced outputs at MVP; regenerating is cheap because the inputs are preserved.
- Parsing action items into checkboxes/calendar integrations is post-MVP polish.
