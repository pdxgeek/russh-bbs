-- M3-A: the Postgres counterpart of the SQLite 0014 migration (layout and theme, D79).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.

ALTER TABLE terminal_settings ADD COLUMN layout TEXT NOT NULL DEFAULT 'classic'
    CHECK (layout IN ('classic', 'lightbar', 'compact'));
ALTER TABLE terminal_settings ADD COLUMN theme TEXT NOT NULL DEFAULT 'default'
    CHECK (theme ~ '^[a-z0-9][a-z0-9_-]{0,23}$');
