-- M3-G (Postgres counterpart of the SQLite 0018 migration, D24): RSS and Atom feeds as read-only
-- boards, and RSS out (D89). Forward-only: once merged this file is never edited.
--
-- A feed is a source URL bound to one board. The board is an ordinary access resource (D83) with
-- `feed = TRUE`, which core's rules read as "nobody posts or replies here". Items become threads
-- with no author account. Everything stored from a feed was cleaned by core's shared sanitizer
-- first; the CHECKs repeat the ceilings so a bug in a caller cannot fill the database. Nothing
-- here is encrypted: the server reads all of it (D3).
--
-- `feed_item` remembers what was already seen, by two hashes, so a removed or aged-out thread is
-- not fetched back and a republished item under a new id is not posted twice. It keeps no text.
-- `board_public` is the sysop's explicit "this board may be read as RSS by outside readers"; it is
-- the one input to the anonymous snapshot RSS out resolves against (D89).

ALTER TABLE board ADD COLUMN feed BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE feed (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    board_id        BIGINT NOT NULL UNIQUE REFERENCES board (id) ON DELETE CASCADE,
    url             TEXT NOT NULL UNIQUE CHECK (char_length(url) BETWEEN 8 AND 2048),
    -- Plain http is allowed for this feed only because the sysop said so.
    allow_http      BOOLEAN NOT NULL,
    interval_secs   BIGINT NOT NULL CHECK (interval_secs BETWEEN 900 AND 604800),
    created_by      BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    created_at      BIGINT NOT NULL,
    last_attempt_at BIGINT,
    last_success_at BIGINT,
    -- A closed set; never an error message (a remote server's words are not stored).
    last_outcome    TEXT CHECK (last_outcome IN
                        ('ok', 'refused', 'failed', 'invalid', 'too-large', 'timed-out')),
    failures        BIGINT NOT NULL DEFAULT 0 CHECK (failures >= 0)
);

CREATE TABLE feed_item (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    feed_id      BIGINT NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    -- SHA-256 hex of the item's guid, or its link when it has none.
    key_hash     TEXT NOT NULL CHECK (char_length(key_hash) = 64),
    -- SHA-256 hex of the cleaned title, link and body.
    content_hash TEXT NOT NULL CHECK (char_length(content_hash) = 64),
    -- The thread it became; NULL once the thread was removed or aged out.
    thread_id    BIGINT REFERENCES thread (id) ON DELETE SET NULL,
    seen_at      BIGINT NOT NULL,
    UNIQUE (feed_id, key_hash),
    UNIQUE (feed_id, content_hash)
);

CREATE INDEX feed_item_by_seen ON feed_item (feed_id, seen_at, id);

CREATE TABLE board_public (
    board_id     BIGINT PRIMARY KEY REFERENCES board (id) ON DELETE CASCADE,
    published_by BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    published_at BIGINT NOT NULL
);
