-- M2-E: the sysop queue of invite requests, vouches, and the minimal memory of a denial (D71,
-- D72). Forward-only: once merged this file is never edited (see src/sqlite/tests.rs).
-- Numbered 0009; 0008 is reserved for M2-B2.
--
-- Minimal data (docs/threat-model.md, "Invites, registration, and abuse"): a request holds the
-- sanitized handle and message, the presenting key's public fingerprint and two timestamps. No
-- address, no user agent, nothing else. A denial deletes the request and keeps only a hash of
-- the fingerprint and an expiry. Both tables are purged when their rows expire.

-- `permission` rows for the new waiting-room action `invite.request` (the request-an-invite
-- form). `sysop` always holds every permission (a store test fails without its row); `waiting`
-- is the role an unknown key gets. Nobody else needs it: a member never sees the form.
INSERT INTO role_permission (role, permission) VALUES
    ('waiting', 'invite.request'),
    ('sysop',   'invite.request');

-- A request for an invite. At most one per key (`fingerprint` is unique). Times are the
-- store's clock in Unix seconds, written explicitly so tests control them. `expires_at` is when
-- the row is deleted if nobody decides.
CREATE TABLE invite_request (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    fingerprint TEXT    NOT NULL UNIQUE,
    handle      TEXT    NOT NULL CHECK (length(handle) BETWEEN 1 AND 64),
    message     TEXT    NOT NULL CHECK (length(message) <= 1000),
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL CHECK (expires_at > created_at)
) STRICT;
CREATE INDEX invite_request_by_expiry ON invite_request (expires_at);

-- A member's name on a request. One per member per request; gone with the request (approved,
-- denied or expired) and with the member's account.
CREATE TABLE invite_request_vouch (
    request_id INTEGER NOT NULL REFERENCES invite_request (id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (request_id, account_id)
) STRICT;
CREATE INDEX invite_request_vouch_by_account ON invite_request_vouch (account_id);

-- All that remains of a denial: SHA-256 (domain-separated, 64 hex characters) of the key's
-- fingerprint, and when to forget it. Not the fingerprint, not the handle, not the message.
CREATE TABLE request_denial (
    key_hash   TEXT    PRIMARY KEY CHECK (length(key_hash) = 64),
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX request_denial_by_expiry ON request_denial (expires_at);
