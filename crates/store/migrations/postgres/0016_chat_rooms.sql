-- M3-F: the Postgres counterpart of the SQLite 0016 migration (chat room definitions, D84, D78).
-- Forward-only: once merged this file is never edited (see src/postgres/tests.rs). Same table
-- and constraints; Postgres-native types. Nothing said in a room is stored. A room is an access
-- resource (`resource_kind = 'room'`, id = this row's id). No `role_permission` rows: the chat
-- permissions exist since 0003.
--
-- GENERATED ALWAYS AS IDENTITY never reuses an id, so a grant that names a closed room can never
-- point at a later one.
CREATE TABLE chat_room (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name       TEXT   NOT NULL CHECK (length(name) BETWEEN 1 AND 32),
    created_at BIGINT NOT NULL,
    created_by BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    closed_at  BIGINT,
    CHECK (closed_at IS NULL OR closed_at >= created_at)
);

CREATE UNIQUE INDEX chat_room_open_name ON chat_room (name) WHERE closed_at IS NULL;

-- Every board starts with one room so chat works before staff make any. Staff may close it.
INSERT INTO chat_room (name, created_at) VALUES ('lobby', extract(epoch FROM now())::bigint);
