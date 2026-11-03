-- M3-A: the member's layout and theme choice (D82).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.

-- Existing rows get the defaults (the classic layout, the default theme), which is what the
-- screens drew before the choice existed. The allowed values are enforced in core too; the
-- CHECKs only keep nonsense out of the table. A theme name is a bounded identifier, so a stored
-- name needs no sanitizing when it is shown.
ALTER TABLE terminal_settings ADD COLUMN layout TEXT NOT NULL DEFAULT 'classic'
    CHECK (layout IN ('classic', 'lightbar', 'compact'));
ALTER TABLE terminal_settings ADD COLUMN theme TEXT NOT NULL DEFAULT 'default'
    CHECK (length(theme) BETWEEN 1 AND 24 AND theme NOT GLOB '*[^a-z0-9_-]*' AND theme NOT GLOB '[_-]*');
