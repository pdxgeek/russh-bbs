-- M3-F: chat room definitions (D84). Forward-only: once merged this file is never edited (see
-- src/sqlite/tests.rs).
--
-- A room is a row with a name. **Nothing said in a room is stored (D78):** there is no table for
-- lines or whispers, and the audit log gets one row when staff open or close a room and none
-- for anything said. A room is an access resource (`resource_kind = 'room'`, id = this row's
-- id), so who may join, speak or whisper in it is `access_grant` rows like any other resource.
-- The permissions (`chat.join`, `chat.speak`, `chat.whisper`, `chat.moderate`,
-- `chat.manage_rooms`) already exist since 0003 with their role rows, so this file adds none:
-- `member` joins, speaks and whispers everywhere, `cosysop` and `sysop` manage rooms.
--
-- AUTOINCREMENT: ids are never reused, so a grant that names a closed room can never point at a
-- later one. Closing sets `closed_at` and keeps the row (and so the name's history) for that
-- reason; the unique index makes a name unique only among open rooms.
CREATE TABLE chat_room (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 32),
    created_at INTEGER NOT NULL,
    created_by INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    closed_at  INTEGER,
    CHECK (closed_at IS NULL OR closed_at >= created_at)
) STRICT;

CREATE UNIQUE INDEX chat_room_open_name ON chat_room (name) WHERE closed_at IS NULL;

-- Every board starts with one room so chat works before staff make any. Staff may close it.
INSERT INTO chat_room (name, created_at) VALUES ('lobby', CAST(strftime('%s', 'now') AS INTEGER));
