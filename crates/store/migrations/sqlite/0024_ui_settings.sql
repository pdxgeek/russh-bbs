-- UI-2: motion, colour-depth cap and border choice per member, and a layout and theme that may
-- follow the board's default (NULL), with the seven layouts allowed.
-- Forward-only: earlier migrations are untouched, and once merged this file is never edited.

-- SQLite cannot change a CHECK or drop NOT NULL in place, so the table is rebuilt. Nothing
-- references terminal_settings, so the drop needs no foreign-key pragma. Existing rows keep
-- their explicit layout and theme and get the defaults for the new columns.
CREATE TABLE terminal_settings_new (
    account_id      INTEGER NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    width_override  INTEGER CHECK (width_override  IS NULL OR width_override  > 0),
    height_override INTEGER CHECK (height_override IS NULL OR height_override > 0),
    color           INTEGER NOT NULL CHECK (color IN (0, 1)),
    pause           INTEGER NOT NULL CHECK (pause IN (0, 1)),
    icon_tier       TEXT    CHECK (icon_tier IS NULL OR icon_tier IN ('nerd_font', 'unicode', 'ascii')),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    -- NULL = follow the board's default. A theme name is a bounded identifier.
    layout          TEXT    CHECK (layout IS NULL OR layout IN
                        ('classic', 'lightbar', 'compact', 'dashboard', 'wide', 'focus', 'split')),
    theme           TEXT    CHECK (theme IS NULL OR (length(theme) BETWEEN 1 AND 24
                        AND theme NOT GLOB '*[^a-z0-9_-]*' AND theme NOT GLOB '[_-]*')),
    motion          INTEGER NOT NULL DEFAULT 1 CHECK (motion IN (0, 1)),
    depth_cap       TEXT    NOT NULL DEFAULT 'auto' CHECK (depth_cap IN ('auto', '16', '256', 'truecolor')),
    borders         TEXT    NOT NULL DEFAULT 'theme' CHECK (borders IN ('theme', 'ascii'))
) STRICT;

INSERT INTO terminal_settings_new
    (account_id, width_override, height_override, color, pause, icon_tier, updated_at, layout, theme)
SELECT account_id, width_override, height_override, color, pause, icon_tier, updated_at, layout, theme
FROM terminal_settings;

DROP TABLE terminal_settings;
ALTER TABLE terminal_settings_new RENAME TO terminal_settings;
