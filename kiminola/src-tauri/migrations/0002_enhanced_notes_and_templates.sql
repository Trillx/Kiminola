-- Add persisted AI-enhanced notes and summary templates.

ALTER TABLE notes ADD COLUMN enhanced_markdown TEXT;

CREATE TABLE templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    is_builtin INTEGER NOT NULL DEFAULT 0
);

INSERT INTO templates (name, prompt, is_builtin) VALUES
(
    'General',
    'You are a helpful meeting assistant. Turn the transcript below into a concise, structured set of notes.\n\nUse this format:\n## Summary\n## Action items\n## Decisions\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
),
(
    '1:1',
    'You are documenting a 1:1 meeting. Produce a concise, balanced summary with sections for what each person said, action items, and follow-ups.\n\n## Summary\n## Discussion points\n## Action items\n## Follow-ups\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
),
(
    'Hiring',
    'You are summarizing a hiring interview. Capture candidate fit, key questions/answers, concerns, and next steps.\n\n## Candidate overview\n## Strengths\n## Concerns / open questions\n## Action items\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
),
(
    'Weekly team',
    'You are summarizing a weekly team sync. Capture updates, blockers, and decisions.\n\n## Summary\n## Wins / progress\n## Blockers\n## Action items\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
),
(
    'Customer discovery',
    'You are documenting a customer discovery call. Summarize the problem, current workflow, pain points, and customer requests.\n\n## Customer context\n## Problems / pain points\n## Jobs-to-be-done\n## Action items\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
),
(
    'VC pitch',
    'You are summarizing a VC pitch. Capture the company, problem, solution, traction, team, and next steps.\n\n## Company / one-liner\n## Problem\n## Solution\n## Traction / market\n## Team\n## Ask / next steps\n\nTranscript:\n{transcript}\n\nRaw notes:\n{notes}',
    1
);
