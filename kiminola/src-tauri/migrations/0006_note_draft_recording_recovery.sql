-- Extend note drafts into durable in-progress recording recovery snapshots.
-- JSON is appropriate here because the rows are rewritten as a whole and are
-- promoted into normalized transcript_segments only when a meeting is saved.

ALTER TABLE note_drafts
ADD COLUMN recovery_transcript_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE note_drafts
ADD COLUMN recovery_duration_seconds INTEGER NOT NULL DEFAULT 0;
