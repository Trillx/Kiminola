ALTER TABLE board_cards ADD COLUMN source_action_index INTEGER;

-- Legacy meeting cards predate stable action occurrence links. Mark them for
-- one-time title-based adoption when that action is next edited. New custom
-- meeting cards remain NULL and are never included in cascades.
UPDATE board_cards
SET source_action_index = -1
WHERE meeting_id IS NOT NULL;

CREATE INDEX idx_board_cards_source_action
    ON board_cards(meeting_id, source_action_index)
    WHERE source_action_index IS NOT NULL;
