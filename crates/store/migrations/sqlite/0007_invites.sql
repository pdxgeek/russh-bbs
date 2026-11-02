-- M2-D: invite codes, the invite tree, and the end of the legacy `accounts.role` column (D67).
-- Forward-only: once merged this file is never edited (see src/sqlite/tests.rs).
-- No secrets: an invite is stored as the SHA-256 of its code, never the code (AGENTS.md).
--
-- This is SQLite's documented 12-step table rebuild (https://www.sqlite.org/lang_altertable.html,
-- "Making Other Kinds Of Table Schema Changes"). Steps 1 and 10 are the PRAGMA foreign_keys
-- switch: a PRAGMA is a no-op inside a transaction, and sqlx runs each migration in one, so
-- `SqliteStore::open` runs the migrations on a dedicated connection that has foreign keys OFF and
-- then checks `PRAGMA foreign_key_check` before any pool connection exists. With them on, the
-- `DROP TABLE accounts` below would perform an implicit DELETE FROM accounts and cascade into
-- account_keys, terminal_settings, account_logins, access_grant, group_member and group_closure.
-- Do not run this file on a connection that has foreign keys on.

-- The new shape: no `role` (the authority is a grant, D33/D55), and `invited_by`, the inviter,
-- which is the invite tree. The four states are unchanged, so the CHECK is the same.
CREATE TABLE accounts_new (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    username          TEXT    NOT NULL,
    -- ASCII confusables skeleton of `username` (D17): the uniqueness key.
    username_skeleton TEXT    NOT NULL UNIQUE,
    state             TEXT    NOT NULL CHECK (state IN ('pending', 'approved', 'active', 'suspended')),
    created_at        INTEGER NOT NULL DEFAULT (unixepoch()),
    -- Who invited this account. NULL for the founder and for accounts made some other way. The
    -- name `accounts` is resolved after the rename below, when it is the new table.
    invited_by        INTEGER REFERENCES accounts (id) ON DELETE SET NULL
) STRICT;

INSERT INTO accounts_new (id, username, username_skeleton, state, created_at)
SELECT id, username, username_skeleton, state, created_at FROM accounts;

-- AUTOINCREMENT ids are never reused (D35). The new table's counter starts at the highest id it
-- holds; carry the old counter over so a deleted account's id still cannot be handed out again.
DELETE FROM sqlite_sequence WHERE name = 'accounts_new';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'accounts_new', seq FROM sqlite_sequence WHERE name = 'accounts';

DROP TABLE accounts;
ALTER TABLE accounts_new RENAME TO accounts;

-- An invite. `code_hash` is 64 lowercase hex characters of SHA-256 over a domain prefix and the
-- normalized code; the code itself exists only in the inviter's session. Times are the store's
-- clock (Unix seconds), written explicitly so tests control them.
CREATE TABLE invite (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    code_hash   TEXT    NOT NULL UNIQUE CHECK (length(code_hash) = 64),
    inviter_id  INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL CHECK (expires_at > created_at),
    -- Set once, by the redemption transaction. A code with `redeemed_at` set can never be used.
    redeemed_at INTEGER,
    redeemed_by INTEGER REFERENCES accounts (id) ON DELETE SET NULL
) STRICT;
CREATE INDEX invite_by_inviter ON invite (inviter_id, id);
CREATE INDEX accounts_by_inviter ON accounts (invited_by) WHERE invited_by IS NOT NULL;
