-- M3-E: forums with full-text search (D83). Forward-only: once merged this file is never edited
-- (see src/sqlite/tests.rs).
--
-- A board is an access resource (kind `forum`); who may list, read and post is decided by the
-- access tables (0003), not here. `restricted` only says that the board is NOT reachable through
-- a global holding of forum.read: it needs a grant scoped to this board (D83). Boards, threads
-- and posts hold text that core's shared sanitizer has already cleaned; the CHECKs repeat its
-- ceilings so a bug in a caller cannot fill the database. Nothing here is encrypted: it is
-- stored as the server can read it (D3).
--
-- A thread has no body of its own: its first post is the opening message and carries the
-- subject (replies carry an empty subject), so one full-text index over `post` finds both.
-- Removing a post deletes the row, and the index follows by trigger, so removed text cannot be
-- found. `board_id` is repeated on `post` so search filters by board without a join.

CREATE TABLE board (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 40),
    -- Lower-cased name: two boards cannot differ only by case.
    name_key    TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL CHECK (length(description) <= 160),
    restricted  INTEGER NOT NULL CHECK (restricted IN (0, 1)),
    created_by  INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE thread (
    id           INTEGER PRIMARY KEY,
    board_id     INTEGER NOT NULL REFERENCES board (id) ON DELETE CASCADE,
    subject      TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 120),
    author_id    INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    created_at   INTEGER NOT NULL,
    last_post_at INTEGER NOT NULL
) STRICT;

-- "Threads of a board, newest activity first" reads this index in order.
CREATE INDEX thread_by_board ON thread (board_id, last_post_at DESC, id DESC);

CREATE TABLE post (
    id         INTEGER PRIMARY KEY,
    thread_id  INTEGER NOT NULL REFERENCES thread (id) ON DELETE CASCADE,
    board_id   INTEGER NOT NULL REFERENCES board (id) ON DELETE CASCADE,
    author_id  INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    -- The thread's subject on its opening post, empty on a reply.
    subject    TEXT NOT NULL CHECK (length(subject) <= 120),
    body       TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 16000),
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX post_by_thread ON post (thread_id, id);
-- posts_per_hour counts one author's recent posts.
CREATE INDEX post_by_author ON post (author_id, created_at);

-- Full-text index over subject and body: an external-content FTS5 table, kept in step by
-- triggers. `unicode61` folds case and diacritics and splits on anything that is not a letter or
-- digit.
CREATE VIRTUAL TABLE post_fts USING fts5 (
    subject, body,
    content = 'post', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER post_fts_insert AFTER INSERT ON post BEGIN
    INSERT INTO post_fts (rowid, subject, body) VALUES (new.id, new.subject, new.body);
END;

CREATE TRIGGER post_fts_delete AFTER DELETE ON post BEGIN
    INSERT INTO post_fts (post_fts, rowid, subject, body)
    VALUES ('delete', old.id, old.subject, old.body);
END;
