-- M2-E: the Postgres counterpart of SQLite's 0009 (D24: one migration set per backend). Same
-- tables and constraints, Postgres-native types. Forward-only: once merged this file is never
-- edited (see src/postgres/tests.rs). Numbered 0009; 0008 is reserved for M2-B2.
--
-- Minimal data (docs/threat-model.md, "Invites, registration, and abuse"): a request holds the
-- sanitized handle and message, the presenting key's public fingerprint and two timestamps. No
-- address, no user agent, nothing else. A denial deletes the request and keeps only a hash of
-- the fingerprint and an expiry. Both tables are purged when their rows expire.

-- The new waiting-room permission `invite.request` (D71). `sysop` always holds every
-- permission; `waiting` is the role an unknown key gets.
INSERT INTO role_permission (role, permission) VALUES
    ('waiting', 'invite.request'),
    ('sysop',   'invite.request');

-- A request for an invite. At most one per key (`fingerprint` is unique). Times are the
-- store's clock in Unix seconds. `expires_at` is when the row is deleted if nobody decides.
CREATE TABLE invite_request (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    fingerprint TEXT   NOT NULL UNIQUE,
    handle      TEXT   NOT NULL CHECK (length(handle) BETWEEN 1 AND 64),
    message     TEXT   NOT NULL CHECK (length(message) <= 1000),
    created_at  BIGINT NOT NULL,
    expires_at  BIGINT NOT NULL CHECK (expires_at > created_at)
);
CREATE INDEX invite_request_by_expiry ON invite_request (expires_at);

-- A member's name on a request. One per member per request; gone with the request (approved,
-- denied or expired) and with the member's account.
CREATE TABLE invite_request_vouch (
    request_id BIGINT NOT NULL REFERENCES invite_request (id) ON DELETE CASCADE,
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (request_id, account_id)
);
CREATE INDEX invite_request_vouch_by_account ON invite_request_vouch (account_id);

-- All that remains of a denial: SHA-256 (domain-separated, 64 hex characters) of the key's
-- fingerprint, and when to forget it. Not the fingerprint, not the handle, not the message.
CREATE TABLE request_denial (
    key_hash   TEXT   PRIMARY KEY CHECK (length(key_hash) = 64),
    expires_at BIGINT NOT NULL
);
CREATE INDEX request_denial_by_expiry ON request_denial (expires_at);
