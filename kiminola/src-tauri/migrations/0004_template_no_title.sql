-- Stop built-in templates from prepending a duplicate # Title to enhanced notes.
-- The meeting title is already shown in the page header.

UPDATE templates SET prompt = 'You are a helpful meeting assistant. Turn the transcript below into a concise, structured set of notes. Do not add a top-level title; the meeting title is already shown above.

Use this format:
## Summary
## Action items
## Decisions

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'General' AND is_builtin = 1;

UPDATE templates SET prompt = 'You are documenting a 1:1 meeting. Produce a concise, balanced summary with sections for what each person said, action items, and follow-ups. Do not add a top-level title; the meeting title is already shown above.

## Summary
## Discussion points
## Action items
## Follow-ups

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = '1:1' AND is_builtin = 1;

UPDATE templates SET prompt = 'You are summarizing a hiring interview. Capture candidate fit, key questions/answers, concerns, and next steps. Do not add a top-level title; the meeting title is already shown above.

## Candidate overview
## Strengths
## Concerns / open questions
## Action items

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'Hiring' AND is_builtin = 1;

UPDATE templates SET prompt = 'You are summarizing a weekly team sync. Capture updates, blockers, and decisions. Do not add a top-level title; the meeting title is already shown above.

## Summary
## Wins / progress
## Blockers
## Action items

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'Weekly team' AND is_builtin = 1;

UPDATE templates SET prompt = 'You are documenting a customer discovery call. Summarize the problem, current workflow, pain points, and customer requests. Do not add a top-level title; the meeting title is already shown above.

## Customer context
## Problems / pain points
## Jobs-to-be-done
## Action items

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'Customer discovery' AND is_builtin = 1;

UPDATE templates SET prompt = 'You are summarizing a VC pitch. Capture the company, problem, solution, traction, team, and next steps. Do not add a top-level title; the meeting title is already shown above.

## Company / one-liner
## Problem
## Solution
## Traction / market
## Team
## Ask / next steps

Transcript:
{transcript}

Raw notes:
{notes}'
WHERE name = 'VC pitch' AND is_builtin = 1;
