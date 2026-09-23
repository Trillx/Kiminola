ALTER TABLE board_cards ADD COLUMN source_action_index INTEGER;

CREATE INDEX idx_board_cards_source_action
    ON board_cards(meeting_id, source_action_index)
    WHERE source_action_index IS NOT NULL;
