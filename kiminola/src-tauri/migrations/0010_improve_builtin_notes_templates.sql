-- Refresh built-in prompts for existing installations. Custom templates are untouched.
UPDATE templates
SET prompt = 'Create useful meeting notes in Markdown from the transcript and optional raw notes below.

Write a short summary first. Then include sections for Decisions, Action items, and Open questions only when the meeting contains them. Do not add a top-level title; the meeting title appears above the notes.

For each action item, state the task and include an owner and due date only if the source names them. Keep distinct decisions separate from proposals or ideas. Preserve important names, numbers, and dates as given. If the raw notes and transcript disagree, describe the uncertainty instead of choosing one silently. Do not invent facts or fill gaps with generic advice. Omit empty sections and introductory filler.

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'General' AND is_builtin = 1;

UPDATE templates SET prompt = 'Write balanced notes for a 1:1 meeting in Markdown. Summarize the main topics and what each person said without guessing who spoke when the transcript does not make that clear. Include agreements, concerns, feedback, and follow-ups when stated. Use sections for Summary, Discussion, and Action items as needed. Do not add a top-level title or empty sections. Do not turn suggestions into commitments.

Transcript:
{transcript}

Raw notes:
{notes}' WHERE name = '1:1' AND is_builtin = 1;

UPDATE templates SET prompt = 'Write interview notes in Markdown. Summarize the role-relevant evidence from the candidate''s answers, examples, and questions. Separate observed strengths from concerns or unanswered questions. Include next steps only when discussed. Do not make a hiring recommendation or infer protected personal traits. Use sections for Candidate overview, Evidence, Open questions, and Next steps as needed. Do not add a top-level title or empty sections.

Transcript:
{transcript}

Raw notes:
{notes}' WHERE name = 'Hiring' AND is_builtin = 1;

UPDATE templates SET prompt = 'Write notes for a weekly team meeting in Markdown. Group updates by project or topic where the source supports it. Record progress, blockers, decisions, and action items with stated owners and dates. Keep plans distinct from completed work. Use sections for Summary, Updates, Blockers, Decisions, and Action items as needed. Do not add a top-level title or empty sections.

Transcript:
{transcript}

Raw notes:
{notes}' WHERE name = 'Weekly team' AND is_builtin = 1;

UPDATE templates SET prompt = 'Write customer discovery notes in Markdown. Describe the customer''s current workflow, problems, examples, and desired outcomes in their terms. Distinguish what the customer said from ideas proposed during the call. Record requests and follow-ups without treating them as product commitments. Use sections for Customer context, Current workflow, Problems, Requests, and Follow-ups as needed. Do not add a top-level title or empty sections.

Transcript:
{transcript}

Raw notes:
{notes}' WHERE name = 'Customer discovery' AND is_builtin = 1;

UPDATE templates SET prompt = 'Write notes for a VC pitch in Markdown. Summarize the company, problem, product, business model, traction, team, funding ask, investor questions, and next steps only where discussed. Attribute numbers and claims to the speaker; do not present unverified claims as established facts. Distinguish current traction from forecasts. Use topic headings as needed. Do not add a top-level title or empty sections.

Transcript:
{transcript}

Raw notes:
{notes}' WHERE name = 'VC pitch' AND is_builtin = 1;
