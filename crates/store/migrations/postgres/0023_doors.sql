-- M4-D0: the Postgres counterpart of SQLite's 0023 (D24: one migration set per backend). Same
-- tables and constraints, Postgres-native types. Forward-only: once merged this file is never
-- edited (see src/postgres/tests.rs). See the SQLite file for what each table is for.
CREATE TABLE door_state (
    door           TEXT   NOT NULL PRIMARY KEY CHECK (length(door) BETWEEN 1 AND 24),
    season         BIGINT NOT NULL DEFAULT 1 CHECK (season >= 1),
    world_revision BIGINT NOT NULL DEFAULT 0 CHECK (world_revision >= 0),
    world_version  BIGINT NOT NULL DEFAULT 0 CHECK (world_version >= 0),
    world          BYTEA  NOT NULL DEFAULT ''::bytea CHECK (octet_length(world) <= 262144),
    last_tick_day  BIGINT,
    updated_at     BIGINT NOT NULL
);

CREATE TABLE door_save (
    door       TEXT   NOT NULL CHECK (length(door) BETWEEN 1 AND 24),
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    season     BIGINT NOT NULL CHECK (season >= 1),
    version    BIGINT NOT NULL CHECK (version >= 0),
    revision   BIGINT NOT NULL CHECK (revision >= 1),
    data       BYTEA  NOT NULL CHECK (octet_length(data) <= 65536),
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (door, account_id)
);
CREATE INDEX door_save_by_account ON door_save (account_id);

CREATE TABLE door_bar (
    door       TEXT   NOT NULL CHECK (length(door) BETWEEN 1 AND 24),
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    barred_by  BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    at         BIGINT NOT NULL,
    PRIMARY KEY (door, account_id)
);
CREATE INDEX door_bar_by_account ON door_bar (account_id);

INSERT INTO role_permission (role, permission) VALUES
    ('moderator', 'doors.bar_player'),
    ('cosysop',   'doors.bar_player'),
    ('sysop',     'doors.bar_player'),
    ('cosysop',   'doors.reset_player'),
    ('sysop',     'doors.reset_player'),
    ('cosysop',   'doors.end_season'),
    ('sysop',     'doors.end_season');
