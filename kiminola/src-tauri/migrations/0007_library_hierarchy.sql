-- Library hierarchy: a meeting may be filed directly in a Space or under
-- another Meeting. Spaces already have parent_id from the initial schema.
ALTER TABLE meetings
ADD COLUMN parent_meeting_id INTEGER REFERENCES meetings (id);

CREATE INDEX meetings_parent_meeting_id_idx ON meetings (parent_meeting_id);
CREATE INDEX spaces_parent_id_idx ON spaces (parent_id);

-- Older databases can contain meetings saved before an explicit destination
-- existed. Ensure the named fallback exists, then keep every existing meeting
-- visible in Personal.
INSERT INTO spaces (name, created_at)
SELECT 'Personal', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE NOT EXISTS (SELECT 1 FROM spaces WHERE name = 'Personal');

UPDATE meetings
SET space_id = (SELECT id FROM spaces WHERE name = 'Personal' ORDER BY id LIMIT 1)
WHERE space_id IS NULL AND parent_meeting_id IS NULL;

-- Keep the direct-container invariant true for future SQL writes as well as
-- through the application commands.
CREATE TRIGGER meetings_location_insert_check
BEFORE INSERT ON meetings
WHEN (NEW.space_id IS NULL) = (NEW.parent_meeting_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'meeting must have exactly one direct container');
END;

CREATE TRIGGER meetings_location_update_check
BEFORE UPDATE OF space_id, parent_meeting_id ON meetings
WHEN (NEW.space_id IS NULL) = (NEW.parent_meeting_id IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'meeting must have exactly one direct container');
END;
