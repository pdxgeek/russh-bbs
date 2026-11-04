-- UI-2: the Postgres counterpart of the SQLite 0024 migration (motion, colour-depth cap,
-- border choice, and a layout and theme that may follow the board's default).
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.

ALTER TABLE terminal_settings ALTER COLUMN layout DROP NOT NULL;
ALTER TABLE terminal_settings ALTER COLUMN layout DROP DEFAULT;
ALTER TABLE terminal_settings ALTER COLUMN theme DROP NOT NULL;
ALTER TABLE terminal_settings ALTER COLUMN theme DROP DEFAULT;
ALTER TABLE terminal_settings DROP CONSTRAINT terminal_settings_layout_check;
ALTER TABLE terminal_settings ADD CONSTRAINT terminal_settings_layout_check
    CHECK (layout IS NULL OR layout IN
        ('classic', 'lightbar', 'compact', 'dashboard', 'wide', 'focus', 'split'));
-- terminal_settings_theme_check (from 0014) already accepts NULL: a CHECK passes on NULL.

ALTER TABLE terminal_settings ADD COLUMN motion BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE terminal_settings ADD COLUMN depth_cap TEXT NOT NULL DEFAULT 'auto'
    CHECK (depth_cap IN ('auto', '16', '256', 'truecolor'));
ALTER TABLE terminal_settings ADD COLUMN borders TEXT NOT NULL DEFAULT 'theme'
    CHECK (borders IN ('theme', 'ascii'));
