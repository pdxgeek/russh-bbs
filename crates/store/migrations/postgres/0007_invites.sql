-- M2-D: the Postgres counterpart of SQLite's 0007 (D24: one migration set per backend). Same
-- tables and constraints, Postgres-native types. Forward-only: once merged this file is never
-- edited (see src/postgres/tests.rs). No secrets: an invite is stored as the SHA-256 of its
-- code, never the code.
--
-- Postgres needs no table rebuild: the legacy `role` column (D33 made it a label nothing reads)
-- is dropped in place, and ids, keys, grants and every foreign key to `accounts` are untouched.

ALTER TABLE accounts DROP COLUMN role;

-- Who invited this account (the invite tree). NULL for the founder and for accounts made some
-- other way; if the inviter's account is ever deleted the tree keeps the invitee.
ALTER TABLE accounts
    ADD COLUMN invited_by BIGINT REFERENCES accounts (id) ON DELETE SET NULL;
CREATE INDEX accounts_by_inviter ON accounts (invited_by) WHERE invited_by IS NOT NULL;

-- An invite. `code_hash` is 64 lowercase hex characters of SHA-256 over a domain prefix and the
-- normalized code; the code itself exists only in the inviter's session. Times are the store's
-- clock (Unix seconds), written explicitly so tests control them.
CREATE TABLE invite (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code_hash   TEXT   NOT NULL UNIQUE CHECK (length(code_hash) = 64),
    inviter_id  BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at  BIGINT NOT NULL,
    expires_at  BIGINT NOT NULL CHECK (expires_at > created_at),
    -- Set once, by the redemption transaction. A code with `redeemed_at` set can never be used.
    redeemed_at BIGINT,
    redeemed_by BIGINT REFERENCES accounts (id) ON DELETE SET NULL
);
CREATE INDEX invite_by_inviter ON invite (inviter_id, id);
