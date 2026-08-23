-- Keep the library destination captured when an in-progress recording draft
-- is created so resuming it cannot silently use a newer global destination.
ALTER TABLE note_drafts
ADD COLUMN recovery_location_json TEXT;
