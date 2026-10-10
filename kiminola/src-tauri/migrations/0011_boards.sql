-- Persist user-created Kanban boards, columns, and meeting action-item cards.

CREATE TABLE boards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE board_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    board_id INTEGER NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0)
);

CREATE TABLE board_cards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    column_id INTEGER NOT NULL REFERENCES board_columns (id) ON DELETE CASCADE,
    meeting_id INTEGER REFERENCES meetings (id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    created_at TEXT NOT NULL
);

CREATE INDEX board_columns_board_position_idx
    ON board_columns (board_id, position, id);

CREATE INDEX board_cards_column_position_idx
    ON board_cards (column_id, position, id);

CREATE INDEX board_cards_meeting_idx
    ON board_cards (meeting_id);
