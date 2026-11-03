-- M3-J: one-liners, the graffiti wall, last callers and polls (D86). Forward-only: once merged
-- this file is never edited (see src/sqlite/tests.rs). Numbered 0017; 0015 and 0016 belong to
-- M3-E and M3-F, which run in parallel.
--
-- Everything here is short sanitized text that core's shared sanitizer cleaned before it was
-- written; the CHECKs repeat core's ceilings so a bug in a caller cannot fill the database. All of
-- it is readable by the server and by every member who may see the feature: nothing is encrypted
-- and nothing is called private. The rate limit, the list size, one vote per member and the
-- creator-or-staff close rule are enforced by the writes themselves, in one transaction each.

-- `community.view` opens the Community menu and reads; `community.moderate` removes entries and
-- polls and closes any poll. `sysop` always holds every permission (a store test fails without
-- its row); custom roles get none until a sysop edits them. The waiting role holds neither.
INSERT INTO role_permission (role, permission) VALUES
    ('member',    'community.view'),
    ('trusted',   'community.view'),
    ('moderator', 'community.view'),
    ('cosysop',   'community.view'),
    ('sysop',     'community.view'),
    ('moderator', 'community.moderate'),
    ('cosysop',   'community.moderate'),
    ('sysop',     'community.moderate');

-- One-liners and wall lines share a table; `kind` says which list. A write keeps the newest
-- `keep` lines of its kind (15 and 12) and deletes older ones in the same transaction.
CREATE TABLE community_line (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL CHECK (kind IN ('oneliner', 'graffiti')),
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    body       TEXT    NOT NULL CHECK (length(body) BETWEEN 1 AND 70),
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX community_line_by_kind    ON community_line (kind, id DESC);
CREATE INDEX community_line_by_account ON community_line (account_id, kind, created_at);

-- Last callers: one row per member, updated when a member session starts. Only the time of the
-- latest session is kept, not a history. Joined to `accounts` for the handle and the state, so a
-- suspended or pending account never shows.
CREATE TABLE last_caller (
    account_id INTEGER NOT NULL PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    last_at    INTEGER NOT NULL
) STRICT;
CREATE INDEX last_caller_by_time ON last_caller (last_at DESC);

-- Polls. `results` is when counts show: always, after the viewer voted, or after the poll
-- closes. `closed_at` is NULL while open.
CREATE TABLE poll (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    creator_id  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    question    TEXT    NOT NULL CHECK (length(question) BETWEEN 1 AND 60),
    description TEXT    NOT NULL DEFAULT '' CHECK (length(description) <= 350),
    results     TEXT    NOT NULL CHECK (results IN ('always', 'after_vote', 'after_close')),
    created_at  INTEGER NOT NULL,
    closed_at   INTEGER
) STRICT;
CREATE INDEX poll_by_creator ON poll (creator_id, created_at);

CREATE TABLE poll_option (
    poll_id  INTEGER NOT NULL REFERENCES poll (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 5),
    text     TEXT    NOT NULL CHECK (length(text) BETWEEN 1 AND 40),
    PRIMARY KEY (poll_id, position)
) STRICT;

-- One vote per member per poll: the primary key. The composite foreign key means a vote can only
-- name an option the poll has.
CREATE TABLE poll_vote (
    poll_id    INTEGER NOT NULL REFERENCES poll (id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    voted_at   INTEGER NOT NULL,
    PRIMARY KEY (poll_id, account_id),
    FOREIGN KEY (poll_id, position) REFERENCES poll_option (poll_id, position) ON DELETE CASCADE
) STRICT;
