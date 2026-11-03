-- M3-G: RSS and Atom feeds as read-only boards, and RSS out (D89). Forward-only: once merged this
-- file is never edited (see src/sqlite/tests.rs).
--
-- A feed is a source URL bound to one board. The board is an ordinary access resource (D83) with
-- `feed = 1`, which core's rules read as "nobody posts or replies here". Items become threads with
-- no author account. Everything stored from a feed was cleaned by core's shared sanitizer first;
-- the CHECKs repeat the ceilings so a bug in a caller cannot fill the database. Nothing here is
-- encrypted: the server reads all of it (D3).
--
-- `feed_item` remembers what was already seen, by two hashes, so a removed or aged-out thread is
-- not fetched back and a republished item under a new id is not posted twice. It keeps no text.
-- `board_public` is the sysop's explicit "this board may be read as RSS by outside readers"; it is
-- the one input to the anonymous snapshot RSS out resolves against (D89).

ALTER TABLE board ADD COLUMN feed INTEGER NOT NULL DEFAULT 0 CHECK (feed IN (0, 1));

CREATE TABLE feed (
    id              INTEGER PRIMARY KEY,
    board_id        INTEGER NOT NULL UNIQUE REFERENCES board (id) ON DELETE CASCADE,
    url             TEXT NOT NULL UNIQUE CHECK (length(url) BETWEEN 8 AND 2048),
    -- Plain http is allowed for this feed only because the sysop said so.
    allow_http      INTEGER NOT NULL CHECK (allow_http IN (0, 1)),
    interval_secs   INTEGER NOT NULL CHECK (interval_secs BETWEEN 900 AND 604800),
    created_by      INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    created_at      INTEGER NOT NULL,
    last_attempt_at INTEGER,
    last_success_at INTEGER,
    -- A closed set; never an error message (a remote server's words are not stored).
    last_outcome    TEXT CHECK (last_outcome IN
                        ('ok', 'refused', 'failed', 'invalid', 'too-large', 'timed-out')),
    failures        INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0)
) STRICT;

CREATE TABLE feed_item (
    id           INTEGER PRIMARY KEY,
    feed_id      INTEGER NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    -- SHA-256 hex of the item's guid, or its link when it has none.
    key_hash     TEXT NOT NULL CHECK (length(key_hash) = 64),
    -- SHA-256 hex of the cleaned title, link and body.
    content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
    -- The thread it became; NULL once the thread was removed or aged out.
    thread_id    INTEGER REFERENCES thread (id) ON DELETE SET NULL,
    seen_at      INTEGER NOT NULL,
    UNIQUE (feed_id, key_hash),
    UNIQUE (feed_id, content_hash)
) STRICT;

CREATE INDEX feed_item_by_seen ON feed_item (feed_id, seen_at, id);

CREATE TABLE board_public (
    board_id     INTEGER PRIMARY KEY REFERENCES board (id) ON DELETE CASCADE,
    published_by INTEGER REFERENCES accounts (id) ON DELETE SET NULL,
    published_at INTEGER NOT NULL
) STRICT;
