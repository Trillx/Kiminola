-- The default Space is identified by its stable row id, not by its mutable
-- display name. Seed the setting for existing databases before users can
-- rename the Space through the library UI.
INSERT OR IGNORE INTO settings (key, value)
SELECT 'default_space_id', CAST(id AS TEXT)
FROM spaces
WHERE name = 'Personal'
ORDER BY id
LIMIT 1;
