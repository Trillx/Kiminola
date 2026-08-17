-- Library search: FTS5 index over meeting titles, notes, and transcripts.
-- One document per meeting, keyed by rowid = meeting_id, so updates are
-- idempotent and results are naturally de-duplicated.

CREATE VIRTUAL TABLE search_index USING fts5(body, content='');

-- Backfill existing meetings with their full searchable text.
INSERT INTO search_index (rowid, body)
SELECT
    m.id,
    COALESCE(m.title, '') || ' ' ||
    COALESCE(n.raw_markdown, '') || ' ' ||
    COALESCE(n.enhanced_markdown, '') || ' ' ||
    COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = m.id), '')
FROM meetings m
LEFT JOIN notes n ON n.meeting_id = m.id;

-- Trigger helper: recompute a meeting's searchable text and upsert it.
-- Defined as a view-like expression because SQLite does not support trigger
-- functions, but the same subquery is repeated in each trigger below.

CREATE TRIGGER meetings_search_insert
AFTER INSERT ON meetings
BEGIN
    INSERT INTO search_index (rowid, body)
    SELECT NEW.id,
           COALESCE(NEW.title, '') || ' ' ||
           COALESCE((SELECT raw_markdown FROM notes WHERE meeting_id = NEW.id), '') || ' ' ||
           COALESCE((SELECT enhanced_markdown FROM notes WHERE meeting_id = NEW.id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.id), '');
END;

CREATE TRIGGER meetings_search_update
AFTER UPDATE OF title ON meetings
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT NEW.id,
           COALESCE(NEW.title, '') || ' ' ||
           COALESCE((SELECT raw_markdown FROM notes WHERE meeting_id = NEW.id), '') || ' ' ||
           COALESCE((SELECT enhanced_markdown FROM notes WHERE meeting_id = NEW.id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.id), '');
END;

CREATE TRIGGER meetings_search_delete
AFTER DELETE ON meetings
BEGIN
    DELETE FROM search_index WHERE rowid = OLD.id;
END;

CREATE TRIGGER notes_search_insert
AFTER INSERT ON notes
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT NEW.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = NEW.meeting_id), '') || ' ' ||
           COALESCE(NEW.raw_markdown, '') || ' ' ||
           COALESCE(NEW.enhanced_markdown, '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.meeting_id), '');
END;

CREATE TRIGGER notes_search_update
AFTER UPDATE ON notes
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT NEW.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = NEW.meeting_id), '') || ' ' ||
           COALESCE(NEW.raw_markdown, '') || ' ' ||
           COALESCE(NEW.enhanced_markdown, '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.meeting_id), '');
END;

CREATE TRIGGER notes_search_delete
AFTER DELETE ON notes
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT OLD.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = OLD.meeting_id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = OLD.meeting_id), '');
END;

CREATE TRIGGER segments_search_insert
AFTER INSERT ON transcript_segments
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT NEW.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT raw_markdown FROM notes WHERE meeting_id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT enhanced_markdown FROM notes WHERE meeting_id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.meeting_id), '');
END;

CREATE TRIGGER segments_search_update
AFTER UPDATE OF text ON transcript_segments
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT NEW.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT raw_markdown FROM notes WHERE meeting_id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT enhanced_markdown FROM notes WHERE meeting_id = NEW.meeting_id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = NEW.meeting_id), '');
END;

CREATE TRIGGER segments_search_delete
AFTER DELETE ON transcript_segments
BEGIN
    INSERT OR REPLACE INTO search_index (rowid, body)
    SELECT OLD.meeting_id,
           COALESCE((SELECT title FROM meetings WHERE id = OLD.meeting_id), '') || ' ' ||
           COALESCE((SELECT raw_markdown FROM notes WHERE meeting_id = OLD.meeting_id), '') || ' ' ||
           COALESCE((SELECT enhanced_markdown FROM notes WHERE meeting_id = OLD.meeting_id), '') || ' ' ||
           COALESCE((SELECT group_concat(text, ' ') FROM transcript_segments WHERE meeting_id = OLD.meeting_id), '');
END;
