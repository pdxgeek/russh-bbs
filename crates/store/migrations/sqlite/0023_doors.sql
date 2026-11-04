-- M4-D0: the door framework's tables (D98). Forward-only: once merged this file is never edited
-- (see src/sqlite/tests.rs).
--
-- A door is a game that runs in a session. The board stores nothing about a door except what
-- the framework's save API writes here: one opaque, versioned save per player per door, and one
-- row per door for its season, its optional shared world and the last day its daily tick ran.
-- The blobs are bytes the game owns; the database never looks inside them, and caps their size
-- (the same numbers as `core::doors::MAX_SAVE_BYTES` and `MAX_WORLD_BYTES`).

-- One row per door, made the first time a world is written or a season is ended. `season`
-- starts at 1. `world_revision` counts world writes (0: no world yet) and is the compare-and-swap
-- token. `last_tick_day` is the UTC day number the daily tick was last applied for.
CREATE TABLE door_state (
    door           TEXT    NOT NULL PRIMARY KEY CHECK (length(door) BETWEEN 1 AND 24),
    season         INTEGER NOT NULL DEFAULT 1 CHECK (season >= 1),
    world_revision INTEGER NOT NULL DEFAULT 0 CHECK (world_revision >= 0),
    world_version  INTEGER NOT NULL DEFAULT 0 CHECK (world_version >= 0),
    world          BLOB    NOT NULL DEFAULT x'' CHECK (length(world) <= 262144),
    last_tick_day  INTEGER,
    updated_at     INTEGER NOT NULL
) STRICT;

-- One save per player per door. It goes with the account (the cascade), which is how a deleted
-- member's door data is deleted.
CREATE TABLE door_save (
    door       TEXT    NOT NULL CHECK (length(door) BETWEEN 1 AND 24),
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    season     INTEGER NOT NULL CHECK (season >= 1),
    version    INTEGER NOT NULL CHECK (version >= 0),
    revision   INTEGER NOT NULL CHECK (revision >= 1),
    data       BLOB    NOT NULL CHECK (length(data) <= 65536),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (door, account_id)
) STRICT;
CREATE INDEX door_save_by_account ON door_save (account_id);

-- Staff barred this player from this door. The save is kept.
CREATE TABLE door_bar (
    door       TEXT    NOT NULL CHECK (length(door) BETWEEN 1 AND 24),
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    barred_by  INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    at         INTEGER NOT NULL,
    PRIMARY KEY (door, account_id)
) STRICT;
CREATE INDEX door_bar_by_account ON door_bar (account_id);

-- The staff tools' permissions (D98). `doors.run` already exists (0003). `sysop` always holds
-- every permission.
INSERT INTO role_permission (role, permission) VALUES
    ('moderator', 'doors.bar_player'),
    ('cosysop',   'doors.bar_player'),
    ('sysop',     'doors.bar_player'),
    ('cosysop',   'doors.reset_player'),
    ('sysop',     'doors.reset_player'),
    ('cosysop',   'doors.end_season'),
    ('sysop',     'doors.end_season');
