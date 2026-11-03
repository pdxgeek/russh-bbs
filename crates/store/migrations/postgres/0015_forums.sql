-- M3-E (Postgres counterpart of the SQLite 0015 migration, D24): forums with full-text search
-- (D83). Forward-only: once merged this file is never edited.
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
-- Removing a post deletes the row, and the generated column goes with it, so removed text cannot
-- be found. `board_id` is repeated on `post` so search filters by board without a join.

CREATE TABLE board (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    -- Lower-cased name: two boards cannot differ only by case.
    name_key    TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL CHECK (char_length(description) <= 160),
    restricted  BOOLEAN NOT NULL,
    created_by  BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    created_at  BIGINT NOT NULL
);

CREATE TABLE thread (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    board_id     BIGINT NOT NULL REFERENCES board (id) ON DELETE CASCADE,
    subject      TEXT NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 120),
    author_id    BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    created_at   BIGINT NOT NULL,
    last_post_at BIGINT NOT NULL
);

-- "Threads of a board, newest activity first" reads this index in order.
CREATE INDEX thread_by_board ON thread (board_id, last_post_at DESC, id DESC);

CREATE TABLE post (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    thread_id  BIGINT NOT NULL REFERENCES thread (id) ON DELETE CASCADE,
    board_id   BIGINT NOT NULL REFERENCES board (id) ON DELETE CASCADE,
    author_id  BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    -- The thread's subject on its opening post, empty on a reply.
    subject    TEXT NOT NULL CHECK (char_length(subject) <= 120),
    body       TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 16000),
    created_at BIGINT NOT NULL,
    -- Full-text vector over subject and body. The `simple` configuration does no stemming and
    -- no stop words, so a word means the same here as in the SQLite index.
    search     TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', subject || ' ' || body)) STORED
);

CREATE INDEX post_by_thread ON post (thread_id, id);
-- posts_per_hour counts one author's recent posts.
CREATE INDEX post_by_author ON post (author_id, created_at);
CREATE INDEX post_search ON post USING GIN (search);
